//! 发布层：把 `dist/` 目录推送到 Git 仓库或 FTP/SFTP 服务器。
//!
//! 设计要点：
//! - 凭证只以参数形式传入（[`Credentials`]），不写入配置文件，也不在本 crate 内缓存。
//!   桌面端从系统凭据管理器读取后传进来，见 `docs/deploy.md`。
//! - 上传按「差异同步」执行：比对本地清单与远端状态，只传变化文件。

use std::path::{Path, PathBuf};

use serde::Serialize;

pub mod credentials;
pub mod manifest;

#[cfg(feature = "git")]
pub mod git;

#[cfg(any(feature = "ftp", feature = "sftp"))]
pub mod ftp;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误（{path}）: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("发布目录不存在或为空: {0}。请先执行一次生成。")]
    EmptyOutput(PathBuf),

    #[error("缺少发布配置: {0}")]
    MissingConfig(String),

    #[error("凭证不可用: {0}")]
    Credentials(String),

    #[cfg(feature = "git")]
    #[error("Git 操作失败: {0}")]
    Git(#[from] git2::Error),

    #[cfg(feature = "ftp")]
    #[error("FTP 操作失败: {0}")]
    Ftp(String),

    #[cfg(feature = "sftp")]
    #[error("SFTP 操作失败: {0}")]
    Sftp(#[from] ssh2::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

/// 发布过程中的进度事件，用于驱动界面进度条。
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    /// 当前阶段的可读描述，如「上传 posts/index.html」。
    pub message: String,
    pub current: usize,
    pub total: usize,
}

/// 发布凭证。调用结束即丢弃，不做持久化。
#[derive(Clone)]
pub enum Credentials {
    /// 无需认证（如本地裸仓库、匿名 FTP）。
    None,
    /// Git Token / FTP 密码。
    UserPassword { username: String, password: String },
    /// SSH 私钥。
    SshKey {
        username: String,
        private_key: PathBuf,
        passphrase: Option<String>,
    },
}

impl std::fmt::Debug for Credentials {
    /// 手写实现，避免密码或 Token 被日志打印出来。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::None => f.write_str("Credentials::None"),
            Credentials::UserPassword { username, .. } => {
                write!(
                    f,
                    "Credentials::UserPassword {{ username: {username:?}, password: *** }}"
                )
            }
            Credentials::SshKey { username, .. } => {
                write!(
                    f,
                    "Credentials::SshKey {{ username: {username:?}, key: *** }}"
                )
            }
        }
    }
}

/// 发布结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct DeployReport {
    pub target: String,
    pub uploaded: Vec<String>,
    pub deleted: Vec<String>,
    pub skipped: usize,
    pub duration_ms: u64,
    /// Git 发布时的提交哈希。
    pub commit: Option<String>,
    pub warnings: Vec<String>,
}

/// 去掉 URL 里的凭据部分，供报告、进度消息与日志使用。
///
/// 起因：`DeployReport.target` 与「推送到 …」这类进度消息直接用了用户填的 remote。
/// 只要有人把 remote 写成 `https://user:token@github.com/o/r.git`（这是常见做法），
/// token 就会显示在界面上、随报告落进日志。代码本身没拼凭据，但也没把它摘掉。
///
/// 只处理 `scheme://…@host` 这一种形态；SSH 的 `git@host:path` 里的 `git@`
/// 是用户名不是密钥，原样保留。
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    // `@` 要在第一个 `/` 之前才算 userinfo，否则那是路径里的 @。
    let authority_end = rest.find('/').unwrap_or(rest.len());
    match rest[..authority_end].rfind('@') {
        Some(at) => format!("{scheme}://***@{}", &rest[at + 1..]),
        None => url.to_string(),
    }
}

/// 所有发布通道的统一接口。
pub trait Deployer {
    /// 把 `dist_dir` 的内容发布出去。`progress` 会被多次回调。
    fn deploy(&self, dist_dir: &Path, progress: &mut dyn FnMut(Progress)) -> Result<DeployReport>;

    /// 连接性与凭证检查，不做任何写操作。
    fn check(&self) -> Result<()>;
}

/// 按 `staticsmith.toml` 的 `[deploy]` 段构造发布通道。
///
/// 凭证由调用方（桌面端从系统凭据管理器读取）传入，配置文件里只保存非敏感字段。
pub fn from_config(
    config: &staticsmith_core::SiteConfig,
    credentials: Credentials,
) -> Result<Box<dyn Deployer>> {
    use staticsmith_core::config::DeployKind;

    match config.deploy.r#type {
        DeployKind::None => Err(Error::MissingConfig(
            "deploy.type 为 none，尚未选择发布方式".to_string(),
        )),

        #[cfg(feature = "git")]
        DeployKind::Git => {
            let git = config
                .deploy
                .git
                .as_ref()
                .ok_or_else(|| Error::MissingConfig("[deploy.git]".to_string()))?;
            Ok(Box::new(
                git::GitDeployer::new(&git.remote, &git.branch)
                    .with_commit_message(&git.commit_message)
                    .with_credentials(credentials),
            ))
        }

        #[cfg(feature = "ftp")]
        DeployKind::Ftp => {
            let ftp_config = config
                .deploy
                .ftp
                .as_ref()
                .ok_or_else(|| Error::MissingConfig("[deploy.ftp]".to_string()))?;

            if ftp_config.sftp {
                #[cfg(feature = "sftp")]
                {
                    return Ok(Box::new(
                        ftp::SftpDeployer::new(
                            &ftp_config.host,
                            ftp_config.port,
                            &ftp_config.remote_path,
                        )
                        .with_credentials(credentials),
                    ));
                }
                #[cfg(not(feature = "sftp"))]
                return Err(Error::MissingConfig(
                    "当前构建未启用 sftp 特性，无法使用 SFTP 发布".to_string(),
                ));
            }

            Ok(Box::new(
                ftp::FtpDeployer::new(&ftp_config.host, ftp_config.port, &ftp_config.remote_path)
                    .with_credentials(credentials),
            ))
        }

        #[cfg(not(feature = "git"))]
        DeployKind::Git => Err(Error::MissingConfig("当前构建未启用 git 特性".to_string())),

        #[cfg(not(feature = "ftp"))]
        DeployKind::Ftp => Err(Error::MissingConfig("当前构建未启用 ftp 特性".to_string())),
    }
}

/// 确认输出目录存在且非空——避免把空目录同步上去清空线上站点。
pub(crate) fn ensure_output_ready(dist_dir: &Path) -> Result<()> {
    if !dist_dir.is_dir() {
        return Err(Error::EmptyOutput(dist_dir.to_path_buf()));
    }
    let has_file = walkdir::WalkDir::new(dist_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| e.file_type().is_file());
    if has_file {
        Ok(())
    } else {
        Err(Error::EmptyOutput(dist_dir.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// remote 里带 token 是常见做法，报告与进度消息里不能把它显示出来。
    #[test]
    fn redact_url_strips_credentials() {
        assert_eq!(
            redact_url("https://user:ghp_secret@github.com/o/r.git"),
            "https://***@github.com/o/r.git"
        );
        assert_eq!(
            redact_url("https://ghp_secret@github.com/o/r.git"),
            "https://***@github.com/o/r.git"
        );
    }

    /// 不该动的形态一律原样返回：SSH 的 `git@host` 里那是用户名，不是密钥；
    /// 路径里出现 `@` 也不能被当成 userinfo 截掉。
    #[test]
    fn redact_url_leaves_clean_urls_alone() {
        for url in [
            "https://github.com/o/r.git",
            "git@github.com:o/r.git",
            "https://github.com/o/r@v1.git",
            "/tmp/local/bare.git",
        ] {
            assert_eq!(redact_url(url), url, "{url} 不该被改写");
        }
    }

    #[test]
    fn empty_output_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            ensure_output_ready(dir.path()),
            Err(Error::EmptyOutput(_))
        ));
    }

    #[test]
    fn output_with_files_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        assert!(ensure_output_ready(dir.path()).is_ok());
    }

    #[test]
    fn credentials_debug_never_prints_secrets() {
        let creds = Credentials::UserPassword {
            username: "alice".into(),
            password: "super-secret-token".into(),
        };
        let printed = format!("{creds:?}");
        assert!(printed.contains("alice"));
        assert!(!printed.contains("super-secret-token"));
    }
}
