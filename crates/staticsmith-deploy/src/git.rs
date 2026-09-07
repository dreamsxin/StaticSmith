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

use crate::{ensure_output_ready, Credentials, DeployReport, Deployer, Error, Progress, Result};

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
    fn deploy(&self, dist_dir: &Path, progress: &mut dyn FnMut(Progress)) -> Result<DeployReport> {
        let started = Instant::now();
        ensure_output_ready(dist_dir)?;

        progress(Progress {
            message: "准备产物仓库".to_string(),
            current: 0,
            total: 4,
        });
        let repo = self.open_repo(dist_dir)?;
        let branch_ref = format!("refs/heads/{}", self.branch);
        repo.set_head(&branch_ref)?;

        progress(Progress {
            message: "暂存文件".to_string(),
            current: 1,
            total: 4,
        });
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let uploaded = changed_paths(&repo, &tree)?;
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

        if uploaded.is_empty() && parent.is_some() {
            return Ok(DeployReport {
                target: self.remote.clone(),
                warnings: vec!["产物与上次提交一致，已跳过提交".to_string()],
                duration_ms: started.elapsed().as_millis() as u64,
                ..Default::default()
            });
        }

        progress(Progress {
            message: "提交变更".to_string(),
            current: 2,
            total: 4,
        });
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
            message: format!("推送到 {}", self.remote),
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
            target: self.remote.clone(),
            uploaded,
            deleted: Vec::new(),
            skipped: 0,
            duration_ms: started.elapsed().as_millis() as u64,
            commit: Some(commit_id.to_string()),
            warnings: Vec::new(),
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
        let err = deployer.deploy(dir.path(), &mut |_| {}).err().unwrap();
        assert!(matches!(err, Error::EmptyOutput(_)));
    }

    #[test]
    fn deploy_commits_and_pushes_to_remote() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let mut events = Vec::new();
        let report = deployer
            .deploy(dist.path(), &mut |p| events.push(p.message))
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

        deployer.deploy(dist.path(), &mut |_| {}).unwrap();
        let second = deployer.deploy(dist.path(), &mut |_| {}).unwrap();

        assert!(second.commit.is_none());
        assert_eq!(second.warnings.len(), 1);
    }

    #[test]
    fn changed_file_produces_a_new_commit() {
        let (_remote_dir, remote_url) = bare_remote();
        let dist = dist_with_files();
        let deployer = GitDeployer::new(&remote_url, "main");

        let first = deployer.deploy(dist.path(), &mut |_| {}).unwrap();
        std::fs::write(dist.path().join("index.html"), "<html>changed</html>").unwrap();
        let second = deployer.deploy(dist.path(), &mut |_| {}).unwrap();

        assert_ne!(first.commit, second.commit);
        assert_eq!(second.uploaded, vec!["index.html"]);
    }
}
