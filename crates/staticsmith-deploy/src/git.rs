//! Git 发布：把 `dist/` 目录本身作为一个仓库提交并推送。
//!
//! 之所以不复用项目仓库：站点源码与产物的生命周期不同，
//! 独立仓库让「强制推送产物分支」变成安全操作，也避免污染源码历史。

use std::path::Path;
use std::time::Instant;

use git2::{
    build::CheckoutBuilder, Cred, IndexAddOption, PushOptions, RemoteCallbacks, Repository,
    Signature,
};

use crate::{
    ensure_output_ready, Credentials, DeployPlan, DeployReport, Deployer, Error, Progress,
    ProgressFn, Result,
};

/// Git 发布通道。
pub struct GitDeployer {
    pub remote: String,
    pub branch: String,
    /// 提交信息模板，支持 Tera 语法（如 `站点更新于 {{ now() }}`）。
    pub commit_message: String,
    pub credentials: Credentials,
    /// 远端名称，默认 `origin`。
    pub remote_name: String,
}

impl GitDeployer {
    pub fn new(remote: impl Into<String>, branch: impl Into<String>) -> Self {
        Self {
            remote: remote.into(),
            branch: branch.into(),
            commit_message: "站点更新于 {{ now() }}".to_string(),
            credentials: Credentials::None,
            remote_name: "origin".to_string(),
        }
    }

    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    pub fn with_commit_message(mut self, message: impl Into<String>) -> Self {
        self.commit_message = message.into();
        self
    }

    /// 打开或初始化产物仓库。
    fn open_repo(&self, dist_dir: &Path) -> Result<Repository> {
        match Repository::open(dist_dir) {
            Ok(repo) => Ok(repo),
            // 目录不是仓库根（注意 `open` 不会向上查找，因此不会误开项目仓库）。
            Err(_) => Ok(Repository::init(dist_dir)?),
        }
    }

    fn signature<'a>(&self, repo: &'a Repository) -> Result<Signature<'a>> {
        match repo.signature() {
            Ok(sig) => Ok(sig),
            // 用户没配 user.name/user.email 时给一个明确的机器身份，而不是直接报错。
            Err(_) => Ok(Signature::now("StaticSmith", "staticsmith@localhost")?),
        }
    }

    fn callbacks(&self) -> RemoteCallbacks<'_> {
        let mut callbacks = RemoteCallbacks::new();
        let credentials = self.credentials.clone();
        callbacks.credentials(
            move |_url, username_from_url, _allowed| match &credentials {
                Credentials::None => Cred::default(),
                Credentials::UserPassword { username, password } => {
                    Cred::userpass_plaintext(username, password)
                }
                Credentials::SshKey {
                    username,
                    private_key,
                    passphrase,
                } => Cred::ssh_key(
                    username_from_url.unwrap_or(username),
                    None,
                    private_key,
                    passphrase.as_deref(),
                ),
            },
        );
        callbacks
    }
}

impl Deployer for GitDeployer {
    /// Git 那条路只在**提交之前**能停。
    ///
    /// 提交之后剩下的只有推送这一步，而推送是整个 ref 的原子更新——中间没有
    /// 「传了一半」这种状态，也就没有安全边界可停。停在提交之前则什么都没发生过：
    /// 最多在本地 `dist/.git` 里留下一次暂存，线上一个字节都没变。
    fn deploy(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployReport> {
        let started = Instant::now();
        ensure_output_ready(dist_dir)?;

        let mut flow = progress(Progress {
            message: "准备产物仓库".to_string(),
            current: 0,
            total: 4,
        });
        if flow.is_stop() {
            return Ok(cancelled(&self.remote, started));
        }
        let repo = self.open_repo(dist_dir)?;
        let branch_ref = format!("refs/heads/{}", self.branch);
        repo.set_head(&branch_ref)?;

        flow = progress(Progress {
            message: "暂存文件".to_string(),
            current: 1,
            total: 4,
        });
        if flow.is_stop() {
            return Ok(cancelled(&self.remote, started));
        }
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let uploaded = changed_paths(&repo, &tree)?;
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

        if uploaded.is_empty() && parent.is_some() {
            return Ok(DeployReport {
                target: crate::redact_url(&self.remote),
                warnings: vec!["产物与上次提交一致，已跳过提交".to_string()],
                duration_ms: started.elapsed().as_millis() as u64,
                ..Default::default()
            });
        }

        flow = progress(Progress {
            message: "提交变更".to_string(),
            current: 2,
            total: 4,
        });
        if flow.is_stop() {
            // 最后一个安全边界：暂存区已经写过，但还没有提交、更没有推送。
            return Ok(cancelled(&self.remote, started));
        }
        let signature = self.signature(&repo)?;
        let message = render_commit_message(&self.commit_message);
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let commit_id = repo.commit(
            Some(&branch_ref),
            &signature,
            &signature,
            &message,
            &tree,
            &parents,
        )?;
        // 让工作区状态与新提交一致，避免下次 add_all 报告虚假变更。
        repo.checkout_head(Some(CheckoutBuilder::new().force().remove_untracked(false)))?;

        progress(Progress {
            message: format!("推送到 {}", crate::redact_url(&self.remote)),
            current: 3,
            total: 4,
        });
        let mut remote = match repo.find_remote(&self.remote_name) {
            Ok(existing) => {
                if existing.url() != Some(self.remote.as_str()) {
                    repo.remote_set_url(&self.remote_name, &self.remote)?;
                    repo.find_remote(&self.remote_name)?
                } else {
                    existing
                }
            }
            Err(_) => repo.remote(&self.remote_name, &self.remote)?,
        };

        let mut push_options = PushOptions::new();
        push_options.remote_callbacks(self.callbacks());
        // 产物分支是「当前站点快照」，强制推送是预期行为。
        let refspec = format!("+{branch_ref}:{branch_ref}");
        remote.push(&[refspec.as_str()], Some(&mut push_options))?;

        progress(Progress {
            message: "发布完成".to_string(),
            current: 4,
            total: 4,
        });

        Ok(DeployReport {
            target: crate::redact_url(&self.remote),
            uploaded,
            deleted: Vec::new(),
            skipped: 0,
            duration_ms: started.elapsed().as_millis() as u64,
            commit: Some(commit_id.to_string()),
            warnings: Vec::new(),
            cancelled: false,
        })
    }

    fn plan(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployPlan> {
        ensure_output_ready(dist_dir)?;
        if progress(Progress {
            message: "比对上次提交".to_string(),
            current: 0,
            total: 1,
        })
        .is_stop()
        {
            // 干跑停下来只能是错误：残缺的清单在界面上和完整清单长得一样。
            return Err(Error::Cancelled(
                "发布预览已按要求停止，因此不给出清单：残缺的清单会被当成完整的预览来读"
                    .to_string(),
            ));
        }

        let repo = self.open_repo(dist_dir)?;
        repo.set_head(&format!("refs/heads/{}", self.branch))?;
        // 想知道「相对上次提交变了什么」，只能先暂存一次再 diff：
        // 这会写 dist/.git 里的 index 与 object，但**不提交、不推送**，
        // 也就不影响线上。这件事必须在预览里说出来，不能让「干跑」听起来完全无副作用。
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let tree = repo.find_tree(index.write_tree()?)?;

        let upload = changed_paths(&repo, &tree)?;
        let bytes = upload
            .iter()
            .filter_map(|path| std::fs::metadata(dist_dir.join(path)).ok())
            .map(|meta| meta.len())
            .sum();

        let mut warnings = vec![
            "会更新 dist/.git 里的暂存区以算出差异，但不提交、不推送".to_string(),
            "产物分支是强制推送（+refs/heads/…）：远端该分支上的历史会被这次提交覆盖".to_string(),
        ];
        if upload.is_empty() {
            warnings.push("产物与上次提交一致，这次发布会跳过提交".to_string());
        }

        Ok(DeployPlan {
            target: crate::redact_url(&self.remote),
            upload,
            skipped: 0,
            bytes,
            warnings,
        })
    }

    fn check(&self) -> Result<()> {
        if self.remote.trim().is_empty() {
            return Err(Error::MissingConfig("deploy.git.remote 未填写".to_string()));
        }
        if let Credentials::SshKey { private_key, .. } = &self.credentials {
            if !private_key.is_file() {
                return Err(Error::Credentials(format!(
                    "SSH 私钥不存在: {}",
                    private_key.display()
                )));
            }
        }
        Ok(())
    }
}

/// 提交之前被叫停：线上没有任何变化，这句话必须说出来。
///
/// 与 FTP 那条路不同，Git 停下来时线上**不是**「一半新一半旧」——要么整个 ref 更新，
/// 要么什么都没发生。用户最想知道的正是这个区别。
fn cancelled(remote: &str, started: Instant) -> DeployReport {
    DeployReport {
        target: crate::redact_url(remote),
        warnings: vec!["已按要求停止：还没有提交、也没有推送，线上没有任何变化。\
             本地 dist/.git 里可能留下一次暂存，下次发布会照常覆盖它。"
            .to_string()],
        duration_ms: started.elapsed().as_millis() as u64,
        cancelled: true,
        ..Default::default()
    }
}

/// 相对上一次提交发生变化的文件列表。
fn changed_paths(repo: &Repository, tree: &git2::Tree<'_>) -> Result<Vec<String>> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = repo.diff_tree_to_tree(head_tree.as_ref(), Some(tree), None)?;
    let mut paths = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
                paths.push(path.to_string_lossy().replace('\\', "/"));
            }
            true
        },
        None,
        None,
        None,
    )?;
    paths.sort();
    Ok(paths)
}

/// 渲染提交信息模板。模板出错时退回原始字符串，绝不因为提交信息写错而中断发布。
fn render_commit_message(template: &str) -> String {
    match tera::Tera::one_off(template, &tera::Context::new(), false) {
        Ok(rendered) => rendered,
        Err(err) => {
            tracing::warn!("提交信息模板渲染失败，使用原文: {err}");
            template.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Flow;

    /// 本地裸仓库充当远端，完整跑一遍「提交 + 推送」。
    fn bare_remote() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        Repository::init_bare(dir.path()).unwrap();
        let url = dir.path().to_string_lossy().replace('\\', "/");
        (dir, url)
    }

    fn dist_with_files() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("posts")).unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>home</html>").unwrap();
        std::fs::write(dir.path().join("posts/index.html"), "<html>posts</html>").unwrap();
        dir
    }

    #[test]
    fn commit_message_template_supports_now() {
        let message = render_commit_message("站点更新于 {{ now() }}");
        assert!(message.starts_with("站点更新于 20"), "得到: {message}");
    }

    #[test]
    fn broken_commit_message_template_falls_back_to_raw_text() {
        assert_eq!(render_commit_message("{{ oops("), "{{ oops(");
    }

    #[test]
    fn check_rejects_empty_remote() {
        let deployer = GitDeployer::new("", "main");
        assert!(matches!(deployer.check(), Err(Error::MissingConfig(_))));
    }

    #[test]
    fn check_rejects_missing_ssh_key() {
        let deployer = GitDeployer::new("git@example.com:u/r.git", "main").with_credentials(
            Credentials::SshKey {
                username: "git".into(),
                private_key: "/no/such/key".into(),
                passphrase: None,
            },
        );
        assert!(matches!(deployer.check(), Err(Error::Credentials(_))));
    }

    #[test]
    fn empty_dist_is_refused_before_touching_git() {
        let dir = tempfile::tempdir().unwrap();
        let deployer = GitDeployer::new("file:///tmp/x", "main");
        let err = deployer
            .deploy(dir.path(), &mut |_| Flow::Continue)
            .err()
            .unwrap();
        assert!(matches!(err, Error::EmptyOutput(_)));
    }

    /// 干跑会写 `dist/.git` 的暂存区，所以必须钉住「先干跑再真发」与「直接真发」等价。
    /// 这是加 `plan()` 时唯一靠推理下的结论（diff 的基准是 HEAD 而不是 index），
    /// 推理不该长期代替测试。
    #[test]
    fn planning_first_does_not_change_what_the_real_deploy_does() {
        let (remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let plan = deployer.plan(dist.path(), &mut |_| Flow::Continue).unwrap();
        assert_eq!(plan.upload, vec!["index.html", "posts/index.html"]);
        assert_eq!(plan.bytes, 35, "两个文件的字节数（17 + 18）");
        assert!(
            plan.warnings.iter().any(|w| w.contains("不提交、不推送")),
            "{:?}",
            plan.warnings
        );
        // 干跑不碰远端
        let bare = Repository::open_bare(remote_dir.path()).unwrap();
        assert!(bare.find_reference("refs/heads/main").is_err());

        // 紧接着真发布：结果与「没干跑过」那次一模一样（对照 deploy_commits_and_pushes_to_remote）
        let report = deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();
        assert_eq!(report.uploaded, plan.upload);
        assert!(report.commit.is_some());

        // 再干跑一次：这次该说「与上次提交一致」，而不是把两个文件又报一遍
        let again = deployer.plan(dist.path(), &mut |_| Flow::Continue).unwrap();
        assert!(again.upload.is_empty(), "{:?}", again.upload);
        assert!(
            again.warnings.iter().any(|w| w.contains("跳过提交")),
            "{:?}",
            again.warnings
        );
    }

    #[test]
    fn deploy_commits_and_pushes_to_remote() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let mut events = Vec::new();
        let report = deployer
            .deploy(dist.path(), &mut |p| {
                events.push(p.message);
                Flow::Continue
            })
            .unwrap();

        assert_eq!(report.uploaded, vec!["index.html", "posts/index.html"]);
        assert!(report.commit.is_some());
        assert_eq!(events.len(), 5);

        // 远端确实收到了 main 分支与两个文件。
        let remote_repo = Repository::open_bare(_remote_dir.path()).unwrap();
        let head = remote_repo.find_reference("refs/heads/main").unwrap();
        let tree = head.peel_to_tree().unwrap();
        assert!(tree.get_name("index.html").is_some());
        assert!(tree.get_name("posts").is_some());
    }

    #[test]
    fn second_deploy_without_changes_is_skipped() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();
        let second = deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();

        assert!(second.commit.is_none());
        assert_eq!(second.warnings.len(), 1);
    }

    #[test]
    fn changed_file_produces_a_new_commit() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let first = deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();
        std::fs::write(dist.path().join("index.html"), "<html>changed</html>").unwrap();
        let second = deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();

        assert_ne!(first.commit, second.commit);
        assert_eq!(second.uploaded, vec!["index.html"]);
    }

    /// 提交之前停下来：不提交、不推送，远端连分支都不该出现。
    ///
    /// 「提交变更」那一条进度是最后一个安全边界（第 3 次回调），停在这里
    /// 只在本地留下一次暂存。
    #[test]
    fn stopping_before_the_commit_leaves_the_remote_untouched() {
        let (remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let mut seen = 0;
        let report = deployer
            .deploy(dist.path(), &mut |_| {
                seen += 1;
                if seen >= 3 {
                    Flow::Stop
                } else {
                    Flow::Continue
                }
            })
            .unwrap();

        assert!(report.cancelled, "停止是合法结果，不该变成错误");
        assert!(report.commit.is_none(), "不该提交");
        assert!(report.uploaded.is_empty());
        let warning = report.warnings.first().unwrap();
        assert!(warning.contains("线上没有任何变化"), "{warning}");

        let bare = Repository::open_bare(remote_dir.path()).unwrap();
        assert!(bare.find_reference("refs/heads/main").is_err(), "不该推送");

        // 停过一次之后照常能发出去：上一次只留了暂存，什么都没坏。
        let report = deployer
            .deploy(dist.path(), &mut |_| Flow::Continue)
            .unwrap();
        assert!(!report.cancelled);
        assert!(report.commit.is_some());
    }

    /// 干跑停下来只能是错误：残缺的清单会被当成完整的预览来读。
    #[test]
    fn a_stopped_plan_refuses_to_hand_back_half_a_list() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let err = deployer.plan(dist.path(), &mut |_| Flow::Stop).unwrap_err();

        assert!(matches!(err, Error::Cancelled(_)), "{err:?}");
        assert!(err.to_string().contains("残缺"), "{err}");
    }
}
