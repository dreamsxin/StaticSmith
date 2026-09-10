use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use staticsmith_core::history::Snapshots;
use staticsmith_core::watch::{ChangeSet, ProjectWatcher};
use staticsmith_core::{Builder, PreviewServer, ProjectPaths};
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

/// 取锁，锁被污染时**继续用**里面的值。
///
/// 「污染」的含义是「上一次持锁的线程 panic 了」，不是「里面的数据坏了」。
/// 跟着 panic（`lock().expect(...)`）的代价极不对称：项目还开着、窗口还在，
/// 但此后每一个命令都会在取锁那一行炸，用户只能重启并丢掉未保存的东西。
///
/// 这里装的两类东西都能承受「可能不是最新」：自身写入表是一串带 TTL 的路径，
/// 最坏是多亮一次「检测到外部修改」；`Option<Session>` 的一致性由 `run_writing`
/// 负责（写路径 panic 后立刻重开项目），不靠锁的毒性标记来保证。
///
/// `catch_panics` 仍然是第一道防线——它让锁**不被弄脏**；这个函数管的是
/// 它兜不住的那些地方：文件监听线程、以及持锁期间调用的其他代码。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("锁曾在 panic 中被污染，继续使用其中的值");
        poisoned.into_inner()
    })
}

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
        let mut guard = lock_or_recover(&self.files);
        let now = Instant::now();
        guard.retain(|(_, at)| now.duration_since(*at) < SELF_WRITE_TTL);
        guard.push((Self::key(path), now));
    }

    /// 登记一整棵子树。与单文件登记不同，命中不消费——一次移动会产生很多事件。
    fn note_tree(&self, dir: &Path) {
        let mut guard = lock_or_recover(&self.trees);
        let now = Instant::now();
        guard.retain(|(_, at)| now.duration_since(*at) < SELF_TREE_TTL);
        guard.push((Self::key(dir), now));
    }

    /// 命中即消费掉：同一路径的下一次变更仍应被当作外部改动。
    fn take(&self, path: &Path) -> bool {
        let mut guard = lock_or_recover(&self.files);
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
        let guard = lock_or_recover(&self.trees);
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

        *lock_or_recover(&self.session) = Some(Session {
            root: root.to_path_buf(),
            builder,
            preview: None,
            mcp: None,
            _watcher: Some(watcher),
        });
        Ok(())
    }

    pub fn close(&self) {
        *lock_or_recover(&self.session) = None;
    }

    /// 以只读方式访问当前项目。
    pub fn with_session<T>(&self, f: impl FnOnce(&Session) -> Result<T>) -> Result<T> {
        let guard = lock_or_recover(&self.session);
        let session = guard.as_ref().ok_or(AppError::NoProject)?;
        match catch_panics(|| f(session)) {
            Ok(result) => result,
            // 读路径没动过磁盘，这句话是真的
            Err(message) => Err(panic_error(&message, "磁盘上的内容没有受影响")),
        }
    }

    /// 以可写方式访问当前项目，**并在动手之前留一份内容快照**。
    ///
    /// 界面上的批量动作、删除、栏目改名、跨文件替换都是不可逆的：改的是磁盘上的
    /// 源文件，没有撤销栈。MCP 那侧的安全网挂在 `tools::call` 上——那是个天然的
    /// 唯一分派点，界面这边没有等价物，所以做成一个**必须用它才能写**的访问器：
    /// 需求写在方法名上，而不是靠每个命令记得多调一行。
    ///
    /// 快照与写入在**同一次加锁**里完成。分成两次（先取锁快照、放开、再取锁写入）
    /// 会留一个窗口：内嵌的 MCP 服务端可以在中间插进来改文件，那样快照对应的
    /// 就不是「这次操作之前」的状态了。
    ///
    /// 快照失败不阻断操作，只记日志：没有安全网也比「因为 git 出问题就不让删内容」好。
    pub fn with_writing_session<T>(
        &self,
        operation: &str,
        f: impl FnOnce(&mut Session) -> Result<T>,
    ) -> Result<T> {
        let mut guard = lock_or_recover(&self.session);
        if let Some(session) = guard.as_ref() {
            // 这一句在锁作用域内、又不在 `f` 里，所以 `run_writing` 的兜底盖不到它。
            // libgit2 里的意外 panic 会穿过 guard 把锁弄脏，虽然之后取锁能恢复
            // （`lock_or_recover`），但保持锁干净仍然是第一道防线。
            let paths = session.builder.paths.clone();
            let _ = catch_panics(|| snapshot_before(&paths, operation));
        }
        run_writing(&mut guard, f)
    }

    /// 以可写方式访问当前项目（构建、reload 需要 `&mut Builder`）。
    ///
    /// 会写源文件的命令请改用 [`AppState::with_writing_session`]——这个方法不留快照。
    pub fn with_session_mut<T>(&self, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut guard = lock_or_recover(&self.session);
        run_writing(&mut guard, f)
    }
}

/// 写入访问的公共部分：兜住 panic，并在 panic 后重开项目。
///
/// 取锁留给调用方，这样「先快照再写入」能在一次加锁里做完。
///
/// panic 在这里被兜住并**立刻重开项目**：写入路径上的 panic 可能把 `Builder`
/// 停在改了一半的状态上，拿着半套内存状态继续生成产物比报错更糟。
fn run_writing<T>(
    slot: &mut Option<Session>,
    f: impl FnOnce(&mut Session) -> Result<T>,
) -> Result<T> {
    let session = slot.as_mut().ok_or(AppError::NoProject)?;
    let message = match catch_panics(|| f(session)) {
        Ok(result) => return result,
        Err(message) => message,
    };

    // 重开也要兜住 panic。第一次 panic 很可能来自模板或内容的解析，
    // 而重开正是把同一份磁盘内容再解析一遍——同一个 panic 会再来一次。
    // 那一次若穿过调用方的 `MutexGuard`，Mutex 会被标记 poisoned，
    // 此后每个操作都会再炸，也就是这段代码本来要防的那件事。
    let root = session.root.clone();
    match catch_panics(|| Builder::open(&root)) {
        Ok(Ok(builder)) => {
            session.builder = builder;
            Err(panic_error(&message, "项目状态已从磁盘重新载入"))
        }
        // 重开失败：留着半套状态比没有状态更危险，直接关掉，让用户重开项目
        Ok(Err(err)) => {
            tracing::error!("panic 后重开项目失败: {err}");
            *slot = None;
            Err(panic_error(&message, "项目已关闭，请重新打开"))
        }
        Err(second) => {
            tracing::error!("panic 后重开项目时又 panic 了: {second}");
            *slot = None;
            Err(panic_error(&message, "项目已关闭，请重新打开"))
        }
    }
}

/// 动手之前留一份内容快照。
///
/// 与 MCP 那侧同一套机制（`staticsmith_core::history`），所以界面与 Agent 共用
/// 一条历史：Agent 回退掉的东西，界面这边留下的那一笔也在里面。
///
/// 失败只记日志。事后发现某次改动没有可回退的版本时，原因得查得到。
fn snapshot_before(paths: &ProjectPaths, operation: &str) {
    match Snapshots::open(paths).and_then(|s| s.snapshot(operation)) {
        Ok(Some(snapshot)) => tracing::info!("{operation} 之前已留快照 {}", snapshot.id),
        // 内容与上次快照一致，不留空提交
        Ok(None) => {}
        Err(err) => tracing::warn!("{operation} 之前的快照没留下，这次改动将无法回退: {err}"),
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
/// 调用方兑现——`with_session_mut` 捕获后会重开项目，不带着半套状态继续用。
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

/// panic 兜成的错误信息。
///
/// `aftermath` 必须由调用方给：读路径确实没动磁盘，而写路径可能已经写了一半
/// （`rename_section` 是「先搬整棵目录、再逐篇补 aliases」，批量改标签逐篇落盘）。
/// 对写失败说「磁盘上的内容没有受影响」会让人不去检查、不去回退。
fn panic_error(message: &str, aftermath: &str) -> AppError {
    tracing::error!("命令内部 panic：{message}（{aftermath}）");
    AppError::Message(format!(
        "内部错误：{message}。这是程序缺陷，请附日志反馈；{aftermath}"
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

    /// 锁真的被污染之后，窗口也不能就此变成废的。
    ///
    /// 上面那条测的是「别把锁弄脏」，这条测的是「脏了也得能用」。两者都需要：
    /// `catch_panics` 只覆盖 `f` 本身，而持锁期间还有别的代码在跑
    /// （`with_writing_session` 里的 `snapshot_before` 就在锁作用域内），
    /// 何况文件监听线程完全不在兜底范围内。一旦污染，旧写法的每次
    /// `lock().expect(...)` 都会再 panic：项目还开着，但任何操作都炸，只能重启。
    #[test]
    fn a_poisoned_state_lock_does_not_kill_the_window() {
        let state = AppState::default();

        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.session.lock().unwrap();
            panic!("模拟持锁期间的 panic");
        }));
        assert!(poisoned.is_err());
        assert!(state.session.is_poisoned(), "这条测试的前提就是锁已被污染");

        // 没有项目时应当得到 NoProject，而不是跟着 panic。
        let err = state.with_session(|_| Ok(())).unwrap_err();
        assert!(matches!(err, AppError::NoProject), "{err:?}");
        assert!(matches!(
            state.with_session_mut(|_| Ok(())).unwrap_err(),
            AppError::NoProject
        ));
        state.close();
    }

    /// 自身写入表被污染后，界面不能从此把自己的保存都当成外部改动。
    ///
    /// 这张表由 IPC 线程与**文件监听线程**共用，而监听线程不在 `catch_panics`
    /// 覆盖范围内（`lib.rs` 已自陈）。污染之后旧写法会让每次保存都在这里炸，
    /// 而表里装的只是一串带 TTL 的路径——用一份可能过期的登记远好于让保存全废。
    #[test]
    fn a_poisoned_self_write_table_still_cancels_out_own_writes() {
        let writes = SelfWrites::default();

        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = writes.files.lock().unwrap();
            panic!("模拟监听线程持锁期间的 panic");
        }));
        assert!(poisoned.is_err());
        assert!(writes.files.is_poisoned());

        writes.note(Path::new("content/posts/a.md"));
        let mut set = change_set(&["content/posts/a.md"]);
        assert!(writes.filter(&mut set), "自身写入应当被对消掉");
    }

    /// 动手之前那份快照真的能把内容找回来。
    ///
    /// 断言的是 `snapshot_before` 这一环：留下一笔、提交信息是操作标识符（界面据此
    /// 翻译成人话）、并且真能还原。`with_writing_session` 需要一个 `Session`
    /// （进而需要 Tauri `Window`），在单元测试里造不出来，所以**没有**覆盖到
    /// 「某个命令是不是记得用了带快照的那个访问器」——那一层目前只有编译期的
    /// 「方法名写在那里」作为约束。
    #[test]
    fn a_snapshot_taken_before_a_write_can_be_restored() {
        use staticsmith_core::config::{Assets, Build};
        use staticsmith_core::history::Snapshots;

        let dir = tempfile::tempdir().expect("临时目录");
        std::fs::create_dir_all(dir.path().join("content")).unwrap();
        std::fs::write(
            dir.path().join("staticsmith.toml"),
            "[site]\ntitle = \"t\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("content/a.md"), "原文\n").unwrap();

        let paths = ProjectPaths::new(dir.path(), &Build::default(), &Assets::default());
        snapshot_before(&paths, "batch_delete");

        // 那个操作把文章删了——这正是没有安全网时找不回来的那种改动
        std::fs::remove_file(dir.path().join("content/a.md")).unwrap();

        let snapshots = Snapshots::open(&paths).unwrap();
        let listed = snapshots.list(10).unwrap();
        assert_eq!(listed.len(), 1, "动手之前必须留下一份：{listed:?}");
        assert_eq!(
            listed[0].message, "batch_delete",
            "提交信息就是操作标识符，界面据此翻译成人话"
        );

        snapshots.restore(&listed[0].id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("content/a.md")).unwrap(),
            "原文\n",
            "删掉的文章要能回来"
        );
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
