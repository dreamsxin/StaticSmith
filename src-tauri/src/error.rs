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

impl Serialize for AppError {
    /// 前端只需要一条字符串，`invoke` 的 reject 分支直接拿到它。
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
