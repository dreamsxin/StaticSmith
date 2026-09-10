//! 本地内容快照：破坏性操作前留一份可回退的版本。
//!
//! # 为什么需要它
//!
//! 批量替换、批量搬动、删除内容都有干跑预览，但**落盘之后没有撤销**。
//! 而 MCP 把这些写工具开给了 Agent：干跑只在人真的逐条读了预览时才起作用，
//! Agent 连着调十个工具的时候没人在读。所以要有一层「改之前先存一份」。
//!
//! # 为什么用影子仓库而不是用户自己的 git
//!
//! 站点目录很可能已经是用户自己的 git 仓库（选 git 发布的人几乎一定是）。
//! 往里提交会污染他的历史、和他的暂存区打架、让 `git status` 失去意义——
//! 他刻意留着不提交的改动被顺手提交了，这是不可接受的。
//!
//! 所以这里用**影子仓库**：仓库目录是 `.staticsmith/history.git`，
//! 工作树是项目根。两者完全隔离——用户的 `git log` 干净，我们的历史是我们的。
//! 附带在 `.staticsmith/.gitignore` 里写一个 `*`，让整个内部目录对外层仓库
//! 自动隐身（连它自己也被忽略），免得用户把我们的状态提交进他的仓库。
//!
//! # 只跟踪源，不跟踪产物
//!
//! [`TRACKED`] 之外的一切都不进快照。`dist/` 是可重新生成的，
//! 把每次构建的产物都存一份只会让仓库无谓地涨。

use std::path::{Path, PathBuf};

use git2::{
    build::CheckoutBuilder, Commit, IndexAddOption, Repository, RepositoryInitOptions, Signature,
    Sort,
};
use serde::Serialize;

use crate::error::{Error, Result};

/// 进快照的路径（相对项目根）。
///
/// 只有源：配置、内容、模板、主题、静态资源。刻意**不含** `dist/`（可重新生成）
/// 与 `.staticsmith/`（我们自己的状态，含这个仓库本身）。
pub const TRACKED: &[&str] = &[
    "staticsmith.toml",
    "content",
    "templates",
    "themes",
    "static",
];

/// 提交者身份。
///
/// 固定写死而不读用户的 git 配置：没配过 `user.name` 的机器上 libgit2 会直接
/// 报错，而「留个快照」不该因为这种原因失败。这也让历史里一眼看得出是谁写的。
const AUTHOR_NAME: &str = "StaticSmith";
const AUTHOR_EMAIL: &str = "staticsmith@localhost";

/// 一个快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// 短哈希，回滚时传它。
    pub id: String,
    /// 触发这次快照的操作，如「replace_text」。
    pub message: String,
    /// RFC3339 时间戳。
    pub at: String,
}

/// 回滚结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Restored {
    /// 回滚到的那个快照。
    pub restored_to: String,
    /// 回滚**前**的状态被存成了这一笔。
    ///
    /// 「改之前先快照」意味着最后一次操作的结果本身还没进历史，所以回滚要抹掉的
    /// 恰恰是没人存过的那份。不先存一下，回滚就成了第二次不可逆操作。
    pub previous: Option<Snapshot>,
    /// 记录这次回滚本身的新快照。历史只增不改，回滚也是一次前进。
    pub snapshot: Option<Snapshot>,
}

/// 项目的快照仓库。
pub struct Snapshots {
    repo: Repository,
}

impl Snapshots {
    /// 打开（必要时创建）项目的影子仓库。
    ///
    /// 反复调用是安全的：已存在时直接打开，既有的提交不会丢。
    ///
    /// 仓库建成 **bare** 再在内存里挂上工作树，而不是用 `init` 的 `workdir_path`：
    /// 后者会往工作树里写一个 gitlink（项目根出现 `.git` 文件）。那正是要避免的——
    /// 用户已经有自己的仓库时 init 会直接失败（`cannot overwrite gitlink file`），
    /// 没有仓库时又会让项目根凭空看起来像个 git 仓库。
    /// `set_workdir(root, false)` 的 `false` 就是「不要写 gitlink」。
    pub fn open(root: &Path) -> Result<Self> {
        let internal = root.join(".staticsmith");
        std::fs::create_dir_all(&internal).map_err(|e| Error::io(&internal, e))?;
        write_self_ignore(&internal)?;

        let dir = git_dir(root);
        let repo = if dir.join("HEAD").is_file() {
            Repository::open_bare(&dir)?
        } else {
            let mut opts = RepositoryInitOptions::new();
            opts.bare(true).no_reinit(false);
            Repository::init_opts(&dir, &opts)?
        };
        repo.set_workdir(root, false)?;
        configure(&repo)?;
        Ok(Self { repo })
    }

    /// 仓库目录位置。
    pub fn git_dir(root: &Path) -> PathBuf {
        git_dir(root)
    }

    /// 记一次快照。
    ///
    /// 内容与上一次完全相同时返回 `None`——不留空提交，否则历史里全是噪声。
    pub fn snapshot(&self, message: &str) -> Result<Option<Snapshot>> {
        let mut index = self.repo.index()?;
        // add_all 收新增与修改，update_all 收删除。只有两个都做，
        // 「删掉一篇」才会被记进快照，回滚时那篇才回得来。
        index.add_all(TRACKED.iter(), IndexAddOption::DEFAULT, None)?;
        index.update_all(TRACKED.iter(), None)?;
        index.write()?;
        let tree_id = index.write_tree()?;

        let parent = self.head_commit();
        if parent.as_ref().is_some_and(|p| p.tree_id() == tree_id) {
            return Ok(None);
        }

        let tree = self.repo.find_tree(tree_id)?;
        let who = Signature::now(AUTHOR_NAME, AUTHOR_EMAIL)?;
        let parents: Vec<&Commit> = parent.iter().collect();
        let id = self
            .repo
            .commit(Some("HEAD"), &who, &who, message, &tree, &parents)?;

        Ok(Some(self.describe(&self.repo.find_commit(id)?)))
    }

    /// 最近的若干快照，最新在前。
    pub fn list(&self, limit: usize) -> Result<Vec<Snapshot>> {
        // 一次提交都还没有时 push_head 会失败，那不是错误，只是「还没有历史」
        if self.head_commit().is_none() {
            return Ok(Vec::new());
        }
        let mut walk = self.repo.revwalk()?;
        walk.push_head()?;
        walk.set_sorting(Sort::TIME)?;

        let mut out = Vec::new();
        for id in walk.take(limit) {
            let commit = self.repo.find_commit(id?)?;
            out.push(self.describe(&commit));
        }
        Ok(out)
    }

    /// 回滚到某个快照。
    ///
    /// 三步：先把**当前状态**存一笔（回滚要抹掉的正是它，而「改之前先快照」意味着
    /// 它还没进历史），再签出目标快照，最后把这次回滚也记一笔。
    ///
    /// **不动 HEAD**：历史因此只增不改——回滚错了还能再回滚回来，
    /// 而 `reset` 式的实现会让被跳过的那段无从找回。
    ///
    /// 调用方在这之后必须 `Builder::reload()`：磁盘变了，内存里的页面列表、
    /// SQLite 索引和 `dist/` 都还是旧的。
    pub fn restore(&self, id: &str) -> Result<Restored> {
        let object = self.repo.revparse_single(id)?;
        let commit = object.peel_to_commit()?;
        let tree = commit.tree()?;
        let short = short_id(&commit);

        let previous = self.snapshot(&format!("回滚到 {short} 之前的状态"))?;

        let mut checkout = CheckoutBuilder::new();
        // force：快照之后新建的文件要被删掉，否则「回到那一刻」只回了一半。
        // 限定 pathspec，所以 dist/ 与 .staticsmith/ 不受影响。
        checkout.force();
        for path in TRACKED {
            checkout.path(path);
        }
        self.repo
            .checkout_tree(tree.as_object(), Some(&mut checkout))?;

        let snapshot = self.snapshot(&format!("回滚到 {short}"))?;
        Ok(Restored {
            restored_to: short,
            previous,
            snapshot,
        })
    }

    fn head_commit(&self) -> Option<Commit<'_>> {
        self.repo.head().ok()?.peel_to_commit().ok()
    }

    fn describe(&self, commit: &Commit<'_>) -> Snapshot {
        Snapshot {
            id: short_id(commit),
            message: commit.summary().unwrap_or_default().to_string(),
            at: to_rfc3339(commit.time().seconds()),
        }
    }
}

fn git_dir(root: &Path) -> PathBuf {
    root.join(".staticsmith").join("history.git")
}

/// 影子仓库自己的配置，覆盖用户的全局 git 配置。
///
/// `core.autocrlf` 是必须关掉的：Windows 上它在全局配置里常常是 `true`，
/// 签出时会把 LF 换成 CRLF——等于我们悄悄改写了用户每一行的行尾。后果不只是
/// 难看：他自己的 git 会看到满屏改动，内容哈希也全变了，下次构建变成全量重建。
/// 快照的语义是「原样还回去」，不是「按平台习惯规范化」。
fn configure(repo: &Repository) -> Result<()> {
    let mut config = repo.config()?;
    config.set_bool("core.autocrlf", false)?;
    Ok(())
}

/// 让 `.staticsmith/` 对外层仓库整体隐身。
///
/// 目录里放一个只含 `*` 的 `.gitignore`：用户自己的 git 会连这个文件一起忽略，
/// 于是索引数据库、影子仓库都不会被误提交进他的仓库。
fn write_self_ignore(internal: &Path) -> Result<()> {
    let path = internal.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    std::fs::write(&path, "# StaticSmith 的内部状态，不该进任何仓库\n*\n")
        .map_err(|e| Error::io(&path, e))
}

fn short_id(commit: &Commit<'_>) -> String {
    let full = commit.id().to_string();
    full.chars().take(10).collect()
}

fn to_rfc3339(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个带内容的项目骨架。这里不用 scaffold，免得测试跟着版式的文件数漂。
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("content/posts")).unwrap();
        std::fs::write(
            dir.path().join("staticsmith.toml"),
            "[site]\ntitle = \"t\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("content/posts/a.md"), "原文 A\n").unwrap();
        dir
    }

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).unwrap()
    }

    #[test]
    fn snapshot_then_restore_brings_the_old_text_back() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();

        let first = snapshots.snapshot("replace_text 之前").unwrap().unwrap();
        std::fs::write(dir.path().join("content/posts/a.md"), "被改坏了\n").unwrap();

        let restored = snapshots.restore(&first.id).unwrap();
        assert_eq!(restored.restored_to, first.id);
        assert_eq!(read(dir.path(), "content/posts/a.md"), "原文 A\n");
        // 回滚前的状态被存下来了：改坏的那一版还找得回来
        assert!(restored.previous.is_some(), "回滚不能是第二次不可逆操作");
        // 回滚本身也留一笔：历史只增不改
        assert!(restored.snapshot.is_some());
    }

    /// 回滚不能把「回滚掉的那一版」弄丢——否则它自己就成了第二次不可逆操作。
    #[test]
    fn the_state_thrown_away_by_a_restore_is_still_reachable() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        let good = snapshots.snapshot("起点").unwrap().unwrap();

        std::fs::write(dir.path().join("content/posts/a.md"), "写了一半的新版\n").unwrap();
        let restored = snapshots.restore(&good.id).unwrap();
        assert_eq!(read(dir.path(), "content/posts/a.md"), "原文 A\n");

        // 后悔了：回到那个被回滚掉的版本
        let thrown_away = restored.previous.expect("回滚前的状态要有快照");
        snapshots.restore(&thrown_away.id).unwrap();
        assert_eq!(read(dir.path(), "content/posts/a.md"), "写了一半的新版\n");
    }

    /// 删掉的文件也要能回来——这要靠 update_all，光有 add_all 记不下删除。
    #[test]
    fn restore_brings_back_deleted_files() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        let first = snapshots.snapshot("delete_content 之前").unwrap().unwrap();

        std::fs::remove_file(dir.path().join("content/posts/a.md")).unwrap();
        snapshots.snapshot("delete_content").unwrap();

        snapshots.restore(&first.id).unwrap();
        assert_eq!(read(dir.path(), "content/posts/a.md"), "原文 A\n");
    }

    /// 快照之后新建的文件，回滚时要被清掉，否则「回到那一刻」只回了一半。
    #[test]
    fn restore_removes_files_created_after_the_snapshot() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        let first = snapshots.snapshot("起点").unwrap().unwrap();

        std::fs::write(dir.path().join("content/posts/b.md"), "多出来的\n").unwrap();
        snapshots.snapshot("create_content").unwrap();

        snapshots.restore(&first.id).unwrap();
        assert!(!dir.path().join("content/posts/b.md").exists());
    }

    #[test]
    fn nothing_changed_means_no_snapshot() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        assert!(snapshots.snapshot("第一次").unwrap().is_some());
        assert!(
            snapshots.snapshot("没改任何东西").unwrap().is_none(),
            "内容没变就不该留空提交"
        );
    }

    /// 影子仓库不能在项目根建出 `.git`——那会和用户自己的仓库撞在一起。
    #[test]
    fn the_shadow_repo_stays_out_of_the_project_root() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        snapshots.snapshot("第一次").unwrap();

        assert!(
            !dir.path().join(".git").exists(),
            "项目根不能出现 .git，用户的仓库归他自己"
        );
        assert!(dir.path().join(".staticsmith/history.git").is_dir());
        // 内部目录对外层仓库整体隐身
        let ignore = read(dir.path(), ".staticsmith/.gitignore");
        assert!(ignore.contains('*'), "{ignore}");
    }

    /// 用户已经有自己的仓库时，我们的快照不能出现在他的 `git status` 里。
    #[test]
    fn the_users_own_repository_is_left_alone() {
        let dir = project();
        let theirs = Repository::init(dir.path()).unwrap();

        let snapshots = Snapshots::open(dir.path()).unwrap();
        snapshots.snapshot("我们的快照").unwrap();

        // 我们提交了，但用户的仓库里一次提交都没有
        assert!(theirs.head().is_err(), "不能往用户的仓库里提交");
        assert_eq!(snapshots.list(10).unwrap().len(), 1);
    }

    #[test]
    fn produce_and_restore_are_recorded_newest_first() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        snapshots.snapshot("第一步").unwrap();
        std::fs::write(dir.path().join("content/posts/a.md"), "改了\n").unwrap();
        snapshots.snapshot("第二步").unwrap();

        let list = snapshots.list(10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].message, "第二步", "最新在前");
        assert_eq!(list[1].message, "第一步");
        assert!(
            list[0].at.contains('T'),
            "时间戳要是 RFC3339: {}",
            list[0].at
        );
    }

    /// 行尾必须原样还回去。
    ///
    /// Windows 的全局 git 配置里 `core.autocrlf` 常常是 `true`，那会让签出把 LF
    /// 换成 CRLF：内容看着没变，但每一行都改了，用户自己的 git 会看到满屏改动，
    /// 内容哈希也全变，下次构建退化成全量。这条测试盯的就是 `configure` 里那行设置。
    #[test]
    fn line_endings_survive_a_round_trip() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("content/posts/lf.md"), "一\n二\n三\n").unwrap();
        let first = snapshots.snapshot("起点").unwrap().unwrap();

        std::fs::write(dir.path().join("content/posts/lf.md"), "改坏了").unwrap();
        snapshots.restore(&first.id).unwrap();

        let raw = std::fs::read(dir.path().join("content/posts/lf.md")).unwrap();
        assert!(!raw.windows(2).any(|w| w == b"\r\n"), "不能被换成 CRLF");
        assert_eq!(String::from_utf8(raw).unwrap(), "一\n二\n三\n");
    }

    #[test]
    fn listing_an_empty_history_is_not_an_error() {
        let dir = project();
        let snapshots = Snapshots::open(dir.path()).unwrap();
        assert!(snapshots.list(10).unwrap().is_empty());
    }

    /// 产物目录不进快照：它可以重新生成，存一份只会让仓库白涨。
    #[test]
    fn build_output_is_not_tracked() {
        let dir = project();
        std::fs::create_dir_all(dir.path().join("dist")).unwrap();
        std::fs::write(dir.path().join("dist/index.html"), "<html>").unwrap();

        let snapshots = Snapshots::open(dir.path()).unwrap();
        let first = snapshots.snapshot("第一次").unwrap().unwrap();

        // 只改产物不构成新快照
        std::fs::write(dir.path().join("dist/index.html"), "<html>改了").unwrap();
        assert!(snapshots.snapshot("只改了产物").unwrap().is_none());

        // 回滚也不该动产物
        std::fs::write(dir.path().join("content/posts/a.md"), "改内容\n").unwrap();
        snapshots.restore(&first.id).unwrap();
        assert_eq!(read(dir.path(), "dist/index.html"), "<html>改了");
    }
}
