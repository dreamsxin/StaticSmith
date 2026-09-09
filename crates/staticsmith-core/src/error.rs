use std::path::PathBuf;

/// 核心层统一错误类型。
///
/// 所有对外 API 返回 [`Result`]，Tauri 命令层再把它转成前端可读的字符串。
///
/// 第三方错误一律装箱：`toml::de::Error` 与 `tera::Error` 分别有 96 / 72 字节，
/// 不装箱会让 `Result<T>` 的错误分支膨胀到 128 字节以上，拖慢所有正常返回路径。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误（{path}）: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("配置文件解析失败（{path}）: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("配置序列化失败: {0}")]
    ConfigSerialize(#[source] Box<toml::ser::Error>),

    #[error("项目目录无效: {0}")]
    InvalidProject(String),

    #[error("内容文件 front matter 无效（{path}）: {message}")]
    FrontMatter { path: PathBuf, message: String },

    /// `#[source]` 不是可选的：`tera::Error` 的 `Display` 只说「哪个模板渲染失败」，
    /// **缺哪个变量、第几行都在它的 `source()` 里**。不挂 source，IPC 层就没法把
    /// 真正的原因拼给用户，界面上只剩一句「模板错误: Failed to render …」。
    #[error("模板错误: {0}")]
    Template(#[source] Box<tera::Error>),

    #[error("本地索引错误: {0}")]
    Index(#[source] Box<rusqlite::Error>),

    #[error("文件监听错误: {0}")]
    Watch(#[source] Box<notify::Error>),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// 便捷构造：把 [`std::io::Error`] 与出错路径绑定，避免"文件找不到"这类无上下文报错。
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    /// 配置解析失败，附带文件路径。
    pub fn config_parse(path: impl Into<PathBuf>, source: toml::de::Error) -> Self {
        Error::ConfigParse {
            path: path.into(),
            source: Box::new(source),
        }
    }
}

// 手写 From：装箱后无法直接用 thiserror 的 `#[from]`，但 `?` 仍需要自动转换。
impl From<toml::ser::Error> for Error {
    fn from(source: toml::ser::Error) -> Self {
        Error::ConfigSerialize(Box::new(source))
    }
}

impl From<tera::Error> for Error {
    fn from(source: tera::Error) -> Self {
        Error::Template(Box::new(source))
    }
}

impl From<rusqlite::Error> for Error {
    fn from(source: rusqlite::Error) -> Self {
        Error::Index(Box::new(source))
    }
}

impl From<notify::Error> for Error {
    fn from(source: notify::Error) -> Self {
        Error::Watch(Box::new(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 错误类型的体积直接影响每一次 `Result` 返回，回归时应当被发现。
    #[test]
    fn error_stays_small() {
        assert!(
            std::mem::size_of::<Error>() <= 64,
            "Error 膨胀到 {} 字节，检查是否新增了未装箱的第三方错误",
            std::mem::size_of::<Error>()
        );
    }

    #[test]
    fn io_error_keeps_the_path() {
        let err = Error::io(
            "a/b.md",
            std::io::Error::new(std::io::ErrorKind::NotFound, "缺失"),
        );
        assert!(err.to_string().contains("a/b.md"));
    }

    /// 模板错误的真正原因必须能顺着 `source()` 走到。
    ///
    /// IPC 层靠这条链把「缺哪个变量」拼给用户；`Template` 少挂一个 `#[source]`，
    /// 界面上就只剩一句「模板错误: Failed to render …」。
    #[test]
    fn template_error_exposes_its_cause() {
        use std::error::Error as _;

        let cause = tera::Error::msg("Variable `page.titel` not found");
        let err: Error = tera::Error::chain("Failed to render 'pages/post.html'", cause).into();

        let mut found = false;
        let mut current = err.source();
        while let Some(step) = current {
            if step.to_string().contains("page.titel") {
                found = true;
                break;
            }
            current = step.source();
        }
        assert!(found, "source 链里应当能找到真正的原因：{err}");
    }
}
