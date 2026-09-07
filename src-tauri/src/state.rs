use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use staticsmith_core::watch::{ChangeSet, ProjectWatcher};
use staticsmith_core::{Builder, PreviewServer};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, Result};

/// 文件监听事件的前端事件名。
pub const EVENT_PROJECT_CHANGED: &str = "project://changed";
/// 构建进度事件名。
pub const EVENT_BUILD_PROGRESS: &str = "build://progress";
/// 发布进度事件名。
pub const EVENT_DEPLOY_PROGRESS: &str = "deploy://progress";

/// 一个已打开的项目。
pub struct Session {
    pub root: PathBuf,
    pub builder: Builder,
    /// 本地预览服务器，未启动时为 None。随 Session 一起 drop。
    pub preview: Option<PreviewServer>,
    /// 监听器随 Session 一起 drop，从而自动停止后台线程。
    _watcher: Option<ProjectWatcher>,
}

/// 应用全局状态：同一时刻只允许打开一个项目，避免索引与输出目录相互干扰。
#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Session>>,
}

impl AppState {
    /// 打开项目并启动文件监听。
    pub fn open(&self, app: &AppHandle, root: &Path) -> Result<()> {
        let builder = Builder::open(root)?;
        let paths = builder.paths.clone();

        let handle = app.clone();
        let watcher = ProjectWatcher::start(
            &paths.templates,
            &paths.content,
            &paths.theme,
            Duration::from_millis(300),
            move |set: ChangeSet| {
                // 外部编辑器改了模板 → 通知界面刷新组件树并提示重新生成。
                if let Err(err) = handle.emit(EVENT_PROJECT_CHANGED, &set) {
                    tracing::warn!("发送文件变更事件失败: {err}");
                }
            },
        )?;

        *self.session.lock().expect("状态锁被污染") = Some(Session {
            root: root.to_path_buf(),
            builder,
            preview: None,
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
        f(session)
    }

    /// 以可写方式访问当前项目（构建、reload 需要 `&mut Builder`）。
    pub fn with_session_mut<T>(&self, f: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut guard = self.session.lock().expect("状态锁被污染");
        let session = guard.as_mut().ok_or(AppError::NoProject)?;
        f(session)
    }
}
