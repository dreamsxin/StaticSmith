//! 从环境变量读取发布凭证。
//!
//! 桌面端把凭证放系统凭据管理器，CI 与 Agent 环境里没有那套东西，
//! 因此 CLI 与 MCP 服务端统一走环境变量。逻辑放在这里，避免两处各写一份而慢慢分叉。

use staticsmith_core::config::DeployKind;
use staticsmith_core::SiteConfig;

use crate::{Credentials, Error, Result};

/// Git Token（HTTPS 认证）。
pub const ENV_GIT_TOKEN: &str = "STATICSMITH_GIT_TOKEN";
/// SSH 私钥口令（私钥路径仍在配置里的 `ssh_key_path`）。
pub const ENV_SSH_PASSPHRASE: &str = "STATICSMITH_SSH_PASSPHRASE";
/// FTP 密码的默认变量名，可被配置里的 `password_env` 覆盖。
pub const ENV_FTP_PASSWORD_DEFAULT: &str = "FTP_PASSWORD";

/// 按配置里的发布方式，从环境变量组装凭证。
///
/// 缺变量时报错会写出具体要设哪个变量，而不是笼统的「认证失败」。
pub fn from_env(config: &SiteConfig) -> Result<Credentials> {
    match config.deploy.r#type {
        DeployKind::None => Err(Error::MissingConfig(
            "deploy.type 为 none，请先在 staticsmith.toml 里配置发布方式".to_string(),
        )),
        DeployKind::Git => {
            let git = config
                .deploy
                .git
                .as_ref()
                .ok_or_else(|| Error::MissingConfig("[deploy.git]".to_string()))?;
            if git.auth_type == "ssh" {
                let private_key = git.ssh_key_path.clone().ok_or_else(|| {
                    Error::MissingConfig("auth_type = \"ssh\" 时需要填写 ssh_key_path".to_string())
                })?;
                return Ok(Credentials::SshKey {
                    username: "git".to_string(),
                    private_key,
                    passphrase: std::env::var(ENV_SSH_PASSPHRASE).ok(),
                });
            }
            let token = std::env::var(ENV_GIT_TOKEN)
                .map_err(|_| Error::Credentials(format!("请设置环境变量 {ENV_GIT_TOKEN}")))?;
            Ok(Credentials::UserPassword {
                // GitHub / GitLab 的 Token 认证接受任意用户名。
                username: "staticsmith".to_string(),
                password: token,
            })
        }
        DeployKind::Ftp => {
            let ftp = config
                .deploy
                .ftp
                .as_ref()
                .ok_or_else(|| Error::MissingConfig("[deploy.ftp]".to_string()))?;
            let key = ftp
                .password_env
                .as_deref()
                .unwrap_or(ENV_FTP_PASSWORD_DEFAULT);
            let password = std::env::var(key)
                .map_err(|_| Error::Credentials(format!("请设置环境变量 {key}")))?;
            Ok(Credentials::UserPassword {
                username: ftp.username.clone(),
                password,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_config(auth_type: &str) -> SiteConfig {
        let mut config = SiteConfig::default();
        config.deploy.r#type = DeployKind::Git;
        config.deploy.git = Some(staticsmith_core::config::GitDeploy {
            remote: "https://example.com/r.git".into(),
            branch: "main".into(),
            commit_message: "x".into(),
            auth_type: auth_type.into(),
            ssh_key_path: None,
        });
        config
    }

    #[test]
    fn deploy_none_is_rejected_with_a_hint() {
        let err = from_env(&SiteConfig::default()).unwrap_err();
        assert!(err.to_string().contains("deploy.type"));
    }

    #[test]
    fn missing_git_token_names_the_variable() {
        std::env::remove_var(ENV_GIT_TOKEN);
        let err = from_env(&git_config("token")).unwrap_err();
        assert!(err.to_string().contains(ENV_GIT_TOKEN), "{err}");
    }

    #[test]
    fn ssh_auth_requires_a_key_path() {
        let err = from_env(&git_config("ssh")).unwrap_err();
        assert!(err.to_string().contains("ssh_key_path"), "{err}");
    }

    #[test]
    fn ftp_uses_the_configured_env_var_name() {
        let mut config = SiteConfig::default();
        config.deploy.r#type = DeployKind::Ftp;
        config.deploy.ftp = Some(staticsmith_core::config::FtpDeploy {
            host: "ftp.example.com".into(),
            port: 21,
            username: "alice".into(),
            password_env: Some("MY_FTP_SECRET".into()),
            remote_path: "/public_html".into(),
            overwrite: Default::default(),
            sftp: false,
        });

        std::env::remove_var("MY_FTP_SECRET");
        let err = from_env(&config).unwrap_err();
        assert!(err.to_string().contains("MY_FTP_SECRET"), "{err}");
    }
}
