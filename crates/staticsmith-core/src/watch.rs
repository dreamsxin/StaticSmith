use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use notify::{Event, RecursiveMode, Watcher as _};
use serde::Serialize;

use crate::error::Result;

/// 一批合并后的文件变更。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ChangeSet {
    /// 变更的模板文件（绝对路径）。
    pub templates: Vec<PathBuf>,
    /// 变更的内容文件（绝对路径）。
    pub content: Vec<PathBuf>,
    /// 主题与其他被监听文件。
    pub other: Vec<PathBuf>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty() && self.content.is_empty() && self.other.is_empty()
    }
}

/// 项目文件监听器。
///
/// 用户在外部编辑器直接改 `templates/` 时，界面需要据此刷新组件树并提示重新生成（FAQ Q1）。
/// 事件在 `debounce` 窗口内合并，避免保存一次触发多轮重建。
pub struct ProjectWatcher {
    _watcher: notify::RecommendedWatcher,
    _thread: std::thread::JoinHandle<()>,
}

impl ProjectWatcher {
    /// 开始监听。`on_change` 在后台线程中被调用。
    pub fn start<F>(
        template_dir: &Path,
        content_dir: &Path,
        theme_dir: &Path,
        debounce: Duration,
        on_change: F,
    ) -> Result<Self>
    where
        F: Fn(ChangeSet) + Send + 'static,
    {
        let (tx, rx) = channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        for dir in [template_dir, content_dir, theme_dir] {
            if dir.exists() {
                watcher.watch(dir, RecursiveMode::Recursive)?;
            }
        }

        let templates_root = template_dir.to_path_buf();
        let content_root = content_dir.to_path_buf();

        let thread = std::thread::spawn(move || {
            let mut pending = ChangeSet::default();
            loop {
                match rx.recv_timeout(debounce) {
                    Ok(Ok(event)) => {
                        for path in event.paths {
                            let bucket = if path.starts_with(&templates_root) {
                                &mut pending.templates
                            } else if path.starts_with(&content_root) {
                                &mut pending.content
                            } else {
                                &mut pending.other
                            };
                            if !bucket.contains(&path) {
                                bucket.push(path);
                            }
                        }
                    }
                    Ok(Err(err)) => tracing::warn!("文件监听事件错误: {err}"),
                    Err(RecvTimeoutError::Timeout) => {
                        if !pending.is_empty() {
                            let set = std::mem::take(&mut pending);
                            // 回调是调用方给的（桌面端在里面往前端发事件）。它炸了不能
                            // 把监听线程带走：那之后外部改动再也不会提示，而界面上
                            // 什么都不说。
                            crate::util::keep_running("文件变更回调", || on_change(set));
                        }
                    }
                    // 发送端关闭：Watcher 已被 drop，退出线程。
                    Err(RecvTimeoutError::Disconnected) => {
                        if !pending.is_empty() {
                            let set = std::mem::take(&mut pending);
                            crate::util::keep_running("文件变更回调", || on_change(set));
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _watcher: watcher,
            _thread: thread,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn coalesces_events_and_classifies_paths() {
        let dir = tempfile::tempdir().unwrap();
        let templates = dir.path().join("templates");
        let content = dir.path().join("content");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::create_dir_all(&content).unwrap();

        let (tx, rx) = mpsc::channel();
        let _watcher = ProjectWatcher::start(
            &templates,
            &content,
            &dir.path().join("themes"),
            Duration::from_millis(120),
            move |set| {
                let _ = tx.send(set);
            },
        )
        .unwrap();

        std::fs::write(templates.join("a.html"), "x").unwrap();
        std::fs::write(content.join("a.md"), "y").unwrap();

        let set = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!set.is_empty());
        assert!(
            set.templates.iter().any(|p| p.ends_with("a.html"))
                || set.content.iter().any(|p| p.ends_with("a.md"))
        );
    }

    /// 回调里 panic 不能把监听线程带走。
    ///
    /// 回调由调用方提供（桌面端在里面往前端发事件），一旦它炸了，旧写法会让整个
    /// 监听线程结束：此后**外部改动再也不会提示**，而界面上什么都不说，
    /// 用户只会觉得「这个功能好像没了」。这类静默失效比报错难查得多。
    #[test]
    fn a_panicking_callback_does_not_kill_the_watcher() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let templates = dir.path().join("templates");
        let content = dir.path().join("content");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::create_dir_all(&content).unwrap();

        let (tx, rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let _watcher = ProjectWatcher::start(
            &templates,
            &content,
            &dir.path().join("themes"),
            Duration::from_millis(120),
            move |set| {
                // 第一批故意炸，之后的正常送出去。
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("回调第一次就炸了");
                }
                let _ = tx.send(set);
            },
        )
        .unwrap();

        std::fs::write(content.join("a.md"), "1").unwrap();
        // 等第一批被消化（并炸掉）。
        std::thread::sleep(Duration::from_millis(600));
        std::fs::write(content.join("b.md"), "2").unwrap();

        let set = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("第一批炸了之后，第二批变更仍然应该送到");
        assert!(!set.is_empty());
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }
}
