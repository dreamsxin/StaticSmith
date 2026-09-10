use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use staticsmith_core::watch::{ChangeSet, ProjectWatcher};
use staticsmith_core::{Builder, PreviewServer};
use staticsmith_mcp::McpHttpServer;
use tauri::{Emitter, Manager, Window};

use crate::error::{AppError, Result};

/// 文件监听事件的前端事件名。
pub const EVENT_PROJECT_CHANGED: &str = "project://changed";
/// 构建进度事件名。
pub const EVENT_BUILD_PROGRESS: &str = "build://progress";
/// 发布进度事件名。
pub const EVENT_DEPLOY_PROGRESS: &str = "deploy://progress";

/// 自身写入的登记有效期。
///
/// 监听器有 300 ms 去抖，再留出磁盘与 notify 的延迟，2 秒足够且不会长期压住真实的外部改动。
const SELF_WRITE_TTL: Duration = Duration::from_secs(2);

/// 目录级登记的有效期。
///
/// 栏目改名会一次性搬动整棵子树并逐篇补 front matter，事件会零散地陆续到达，
/// 所以给的窗口比单文件宽一些。
const SELF_TREE_TTL: Duration = Duration::from_secs(6);

/// 应用自己刚写过的文件。
///
/// `save_content` / `save_template` 写的正是被监听的目录，监听器分不清是谁改的，
/// 于是每次保存都会给界面弹一条「检测到外部修改」——这条提示本意是提醒有别的编辑器在动文件，
/// 结果变成了保存的副作用。这里登记自身写入，事件到达时对消掉。
#[derive(Default, Clone)]
struct SelfWrites {
    files: Arc<Mutex<Vec<(String, Instant)>>>,
    /// 目录级登记：改栏目名这类动作会牵动整棵子树，逐个文件登记不现实。
    trees: Arc<Mutex<Vec<(String, Instant)>>>,
}

impl SelfWrites {
    /// 大小写与 `\\?\` 前缀都可能与 notify 给出的路径不一致，统一成比较键。
    fn key(path: &Path) -> String {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.to_lowercase()
    }

    fn note(&self, path: &Path) {
        let mut guard = self.files.lock().expect("自身写入表锁被污染");
        let now = Instant::now();
        guard.retain(|(_, at)| now.duration_since(*at) < SELF_WRITE_TTL);
        guard.push((Self::key(path), now));
    }

    /// 登记一整棵子树。与单文件登记不同，命中不消费——一次移动会产生很多事件。
    fn note_tree(&self, dir: &Path) {
        let mut guard = self.trees.lock().expect("自身写入表锁被污染");
        let now = Instant::now();
        guard.retain(|(_, at)| now.duration_since(*at) < SELF_TREE_TTL);
        guard.push((Self::key(dir), now));
    }

    /// 命中即消费掉：同一路径的下一次变更仍应被当作外部改动。
    fn take(&self, path: &Path) -> bool {
        let mut guard = self.files.lock().expect("自身写入表锁被污染");
        let key = Self::key(path);
        let now = Instant::now();
        match guard
            .iter()
            .position(|(k, at)| *k == key && now.duration_since(*at) < SELF_WRITE_TTL)
        {
            Some(index) => {
                guard.remove(index);
                true
            }
            None => false,
        }
    }

    /// 路径是否落在某个仍在有效期内的目录登记之下。
    fn under_noted_tree(&self, path: &Path) -> bool {
        let guard = self.trees.lock().expect("自身写入表锁被污染");
        let key = Self::key(path);
        let now = Instant::now();
        guard.iter().any(|(prefix, at)| {
            now.duration_since(*at) < SELF_TREE_TTL && key.starts_with(prefix.as_str())
        })
    }

    /// 从变更集中剔除自身写入。返回剩余是否为空。
    fn filter(&self, set: &mut ChangeSet) -> bool {
        for bucket in [&mut set.templates, &mut set.content, &mut set.other] {
            bucket.retain(|path| !self.take(path) && !self.under_noted_tree(path));
        }
        set.is_empty()
    }
}

/// 一个已打开的项目。
pub struct Session {
    pub root: PathBuf,
    pub builder: Builder,
    /// 本地预览服务器，未启动时为 None。随 Session 一起 drop。
    pub preview: Option<PreviewServer>,
    /// MCP 服务端（给 AI Agent 用），未启动时为 None。随 Session 一起 drop。
    ///
    /// 它持有**自己那份** `Builder`：Agent 与界面各读各的一份内存状态。
    /// 这不是疏忽——两边共用一个 `Builder` 就得把每次 Agent 调用都塞进界面的锁里，
    /// 一次 `build_site` 会让界面卡住整段时间。代价是 Agent 改完盘之后界面要刷新，
    /// 而这条路已经有人走：文件监听会发现外部改动并给出「重新读取」的横幅。
    pub mcp: Option<McpHttpServer>,
    /// 监听器随 Session 一起 drop，从而自动停止后台线程。
    _watcher: Option<ProjectWatcher>,
}

/// 应用全局状态：同一时刻只允许打开一个项目，避免索引与输出目录相互干扰。
///
/// 多站点的形态是「一窗口一站点」（见 `docs/architecture.md`），
/// 因此这里保留单 session，但所有事件都按窗口标签定向发送，
/// 将来把 `Option<Session>` 换成按标签索引的表即可，不必再改事件层。
#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Session>>,
    self_writes: SelfWrites,
}

impl AppState {
    /// 登记一次自身写盘，避免监听器把它当成外部改动。
    pub fn note_self_write(&self, path: &Path) {
        self.self_writes.note(path);
    }

    /// 登记一整棵自身改动的子树（栏目改名、批量搬动）。
    pub fn note_self_tree(&self, dir: &Path) {
        self.self_writes.note_tree(dir);
    }

    /// 打开项目并启动文件监听。事件只发给发起打开的那个窗口。
    pub fn open(&self, window: &Window, root: &Path) -> Result<()> {
        let builder = Builder::open(root)?;
        let paths = builder.paths.clone();

        let app = window.app_handle().clone();
        let watch_label = window.label().to_string();
        let self_writes = self.self_writes.clone();
        let watcher = ProjectWatcher::start(
            &paths.templates,
            &paths.content,
            &paths.theme,
            Duration::from_millis(300),
            move |mut set: ChangeSet| {
                // 只剩自身写入时什么都不用做，界面不该为自己的保存亮提示。
                if self_writes.filter(&mut set) {
                    return;
                }
                // 外部编辑器改了模板 → 通知界面刷新组件树并提示重新生成。
                if let Err(err) = app.emit_to(watch_label.as_str(), EVENT_PROJECT_CHANGED, &set) {
                    tracing::warn!("发送文件变更事件失败: {err}");
                }
            },
        )?;

        *self.session.lock().expect("状态锁被污染") = Some(Session {
            root: root.to_path_buf(),
            builder,
            preview: None,
            mcp: None,
            _watcher: Some(watcher),
        });
        Ok(())
    }

    pub fn close(&self) {
        *self.session.lock().expect("状态锁被污染") = None;
    }

    /// 以只读方式访问当前项目。
    pub fn with_session<T>(&self, f: impl FnOnce(&Session) -> Result<T>) -> Result<T> {
        let guard = self.session.lock().expect("状态锁被污染");
        let session = guard.as_ref().ok_or(AppError::NoProject)?;
        match catch_panics(|| f(session)) {
            Ok(result) => result,
            Err(message) => Err(panic_error("读取", &message)),
        }
    }

    /// 以可写方式访问当前项目（构建、reload 需要 `&mut Builder`）。
    ///
    /// panic 在这里被兜住并**立刻从磁盘重载**：写入路径上的 panic 可能把 `Builder`
    /// 停在改了一半的状态上，拿着半套内存状态继续生成产物比报错更糟。
    pub fn with_session_mut<T>(&self, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut guard = self.session.lock().expect("状态锁被污染");
        let session = guard.as_mut().ok_or(AppError::NoProject)?;
        match catch_panics(|| f(session)) {
            Ok(result) => result,
            Err(message) => {
                if let Err(err) = session.builder.reload() {
                    tracing::error!("panic 后从磁盘重载项目状态也失败了，请重新打开项目: {err}");
                }
                Err(panic_error("写入", &message))
            }
        }
    }
}

/// 在 IPC 命令边界上兜住 panic，`Err` 分支带回 panic 的消息。
///
/// 两件事都靠它：
///
/// 1. **不丢稿**。release profile 刻意保留 unwind（见 `Cargo.toml`），
///    渲染线程池里的 panic 会被 rayon 转发到调用线程，兜住后编辑器还活着。
/// 2. **不污染状态锁**。panic 若穿过 `MutexGuard`，`Mutex` 会被标记为 poisoned，
///    此后每次 `lock().expect(...)` 都会再 panic——那才是真正没救的。
///    在锁作用域**内部**兜住，guard 正常析构，锁保持干净。
///
/// `AssertUnwindSafe` 是必需的：`&mut Session` 不是 `UnwindSafe`。这个断言由
/// 调用方兑现——`with_session_mut` 捕获后会重载状态，不带着半套状态继续用。
fn catch_panics<T>(f: impl FnOnce() -> T) -> std::result::Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
        if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<非字符串 payload>".to_string()
        }
    })
}

fn panic_error(what: &str, message: &str) -> AppError {
    tracing::error!("{what}项目时发生 panic：{message}");
    AppError::Message(format!(
        "内部错误：{message}。这是程序缺陷，请附日志反馈；磁盘上的内容没有受影响"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change_set(content: &[&str]) -> ChangeSet {
        ChangeSet {
            templates: Vec::new(),
            content: content.iter().map(PathBuf::from).collect(),
            other: Vec::new(),
        }
    }

    /// 这几个 panic 用例会让默认钩子往 stderr 打印现场，测试输出里出现
    /// 「thread panicked」是预期的。不去替换全局钩子：它是进程级的，
    /// 测试并行跑时改它会互相干扰。
    #[test]
    fn catch_panics_passes_values_through() {
        assert_eq!(catch_panics(|| 42), Ok(42));
    }

    #[test]
    fn catch_panics_reports_the_message() {
        let caught = catch_panics(|| panic!("模板索引越界"));
        assert_eq!(caught, Err("模板索引越界".to_string()));
    }

    /// panic 必须在锁作用域**内部**被兜住。
    ///
    /// 否则 `Mutex` 会被标记为 poisoned，之后每一次 `lock().expect(...)`
    /// 都会再 panic——那才是真正救不回来的：项目还开着，但任何操作都会炸。
    #[test]
    fn catching_inside_the_lock_keeps_the_mutex_usable() {
        let lock = Mutex::new(1);
        {
            let mut guard = lock.lock().unwrap();
            let caught = catch_panics(|| {
                *guard += 1;
                panic!("改了一半就炸了");
            });
            assert!(caught.is_err());
        }
        assert!(!lock.is_poisoned(), "锁不能被污染");
        assert_eq!(*lock.lock().unwrap(), 2, "panic 之前的改动仍然可见");
    }

    #[test]
    fn own_writes_are_filtered_out() {
        let writes = SelfWrites::default();
        writes.note(Path::new("/site/content/posts/a.md"));

        let mut set = change_set(&["/site/content/posts/a.md"]);
        assert!(writes.filter(&mut set), "只有自身写入时应判定为空");
        assert!(set.content.is_empty());
    }

    #[test]
    fn external_edits_survive_alongside_own_writes() {
        let writes = SelfWrites::default();
        writes.note(Path::new("/site/content/posts/a.md"));

        let mut set = change_set(&["/site/content/posts/a.md", "/site/content/posts/b.md"]);
        assert!(!writes.filter(&mut set), "还有外部改动就必须通知界面");
        assert_eq!(set.content, vec![PathBuf::from("/site/content/posts/b.md")]);
    }

    #[test]
    fn a_write_is_consumed_once_only() {
        let writes = SelfWrites::default();
        writes.note(Path::new("/site/content/a.md"));

        // 第一次是自己保存造成的；同一文件随后再被改动就是外部编辑器干的了。
        let mut first = change_set(&["/site/content/a.md"]);
        assert!(writes.filter(&mut first));
        let mut second = change_set(&["/site/content/a.md"]);
        assert!(!writes.filter(&mut second));
    }

    #[test]
    fn windows_path_prefix_and_case_do_not_break_matching() {
        let writes = SelfWrites::default();
        writes.note(Path::new(r"\\?\D:\Site\Content\A.md"));

        let mut set = change_set(&[r"d:\site\content\a.md"]);
        assert!(writes.filter(&mut set), "大小写与 \\\\?\\ 前缀不该影响对消");
    }

    #[test]
    fn stale_registrations_expire() {
        let writes = SelfWrites::default();
        let stale = Instant::now()
            .checked_sub(SELF_WRITE_TTL * 2)
            .expect("测试环境的单调时钟应能回溯几秒");
        writes
            .files
            .lock()
            .unwrap()
            .push((SelfWrites::key(Path::new("/site/content/a.md")), stale));

        let mut set = change_set(&["/site/content/a.md"]);
        assert!(!writes.filter(&mut set), "过期登记不该继续压住变更");
    }

    #[test]
    fn a_noted_tree_swallows_every_path_under_it() {
        let writes = SelfWrites::default();
        // 栏目改名：整棵子树的事件都是自己造成的
        writes.note_tree(Path::new("/site/content/blog"));

        let mut set = change_set(&[
            "/site/content/blog/index.md",
            "/site/content/blog/2026/deep.md",
        ]);
        assert!(writes.filter(&mut set), "目录登记应覆盖其下所有文件");

        // 目录登记不消费：同一次移动会产生很多事件
        let mut again = change_set(&["/site/content/blog/index.md"]);
        assert!(writes.filter(&mut again));

        // 目录之外的改动照常上报
        let mut outside = change_set(&["/site/content/notes/a.md"]);
        assert!(!writes.filter(&mut outside));
    }

    #[test]
    fn expired_tree_registrations_stop_swallowing() {
        let writes = SelfWrites::default();
        let stale = Instant::now()
            .checked_sub(SELF_TREE_TTL * 2)
            .expect("测试环境的单调时钟应能回溯几秒");
        writes
            .trees
            .lock()
            .unwrap()
            .push((SelfWrites::key(Path::new("/site/content/blog")), stale));

        let mut set = change_set(&["/site/content/blog/index.md"]);
        assert!(!writes.filter(&mut set), "过期的目录登记同样要失效");
    }
}
