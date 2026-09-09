use serde::{Serialize, Serializer};

/// IPC 层错误。所有底层错误都在这里被压平为「可直接展示给用户的中文消息」。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("尚未打开项目")]
    NoProject,

    #[error("{0}")]
    Core(#[from] staticsmith_core::Error),

    #[error("{0}")]
    Deploy(#[from] staticsmith_deploy::Error),

    #[error("凭据管理器访问失败: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    /// 把整条 source 链拼成一句话。
    ///
    /// 起因是一个具体缺陷：改坏模板之后，界面只显示「模板错误: Failed to render
    /// 'pages/post.html'」——**缺哪个变量、第几行全没了**。因为 `tera::Error` 把真正
    /// 的原因放在 `.source()` 里，而这里以前只发顶层的 `Display`。
    ///
    /// 所以别再用 `self.to_string()`：顶层消息通常只说「哪一步失败了」，
    /// 用户要的是「为什么失败」。
    fn full_message(&self) -> String {
        use std::error::Error as _;

        let mut message = self.to_string();
        let mut current = self.source();
        while let Some(cause) = current {
            let text = cause.to_string();
            // 上一层常把下一层原样嵌进自己的 Display（`#[error("...: {source}")]`），
            // 重复拼一遍只会让消息变长而不是变清楚。
            if !message.contains(&text) {
                message.push('：');
                message.push_str(&text);
            }
            current = cause.source();
        }
        message
    }
}

impl Serialize for AppError {
    /// 前端只需要一条字符串，`invoke` 的 reject 分支直接拿到它。
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.full_message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 底层原因必须出现在最终消息里，否则「模板错误」等于什么都没说。
    #[test]
    fn full_message_includes_the_root_cause() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "找不到 posts/x.md");
        let core = staticsmith_core::Error::io(std::path::Path::new("posts/x.md"), io);
        let err = AppError::Core(core);

        let message = err.full_message();
        assert!(message.contains("找不到 posts/x.md"), "{message}");
    }

    /// 上层已经把下层嵌进自己的 Display 时不重复拼。
    #[test]
    fn full_message_does_not_repeat_nested_text() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "缺文件");
        let core = staticsmith_core::Error::io(std::path::Path::new("a.md"), io);
        let message = AppError::Core(core).full_message();

        assert_eq!(message.matches("缺文件").count(), 1, "{message}");
    }
}
