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
