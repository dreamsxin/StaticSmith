use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// 项目根目录下的配置文件名。
pub const CONFIG_FILE_NAME: &str = "staticsmith.toml";

/// `staticsmith.toml` 的完整映射。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiteConfig {
    pub site: Site,
    #[serde(default)]
    pub build: Build,
    #[serde(default)]
    pub deploy: Deploy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Site {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_language")]
    pub language: String,
    /// 任意扩展字段，原样透传给模板的 `site.extra`。
    #[serde(default)]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Build {
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
    #[serde(default = "default_content_dir")]
    pub content_dir: PathBuf,
    #[serde(default = "default_theme_dir")]
    pub theme_dir: PathBuf,
    /// 统一模板目录（layouts / components / pages）。
    #[serde(default = "default_template_dir")]
    pub template_dir: PathBuf,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub minify: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deploy {
    /// `git` 或 `ftp`。
    #[serde(default)]
    pub r#type: DeployKind,
    #[serde(default)]
    pub git: Option<GitDeploy>,
    #[serde(default)]
    pub ftp: Option<FtpDeploy>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeployKind {
    #[default]
    None,
    Git,
    Ftp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDeploy {
    pub remote: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default = "default_commit_message")]
    pub commit_message: String,
    /// `token` 或 `ssh`。凭证本身不落盘，见 `docs/deploy.md`。
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub ssh_key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpDeploy {
    pub host: String,
    #[serde(default = "default_ftp_port")]
    pub port: u16,
    pub username: String,
    /// 只保存环境变量名；真实密码存放于系统凭据管理器。
    #[serde(default)]
    pub password_env: Option<String>,
    #[serde(default = "default_remote_path")]
    pub remote_path: String,
    /// 为 true 时使用 SFTP（22 端口）而非明文 FTP。
    #[serde(default)]
    pub sftp: bool,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            content_dir: default_content_dir(),
            theme_dir: default_theme_dir(),
            template_dir: default_template_dir(),
            page_size: default_page_size(),
            minify: false,
        }
    }
}

impl SiteConfig {
    /// 从项目根目录读取 `staticsmith.toml`。
    pub fn load(project_root: impl AsRef<Path>) -> Result<Self> {
        let path = project_root.as_ref().join(CONFIG_FILE_NAME);
        let raw = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        toml::from_str(&raw).map_err(|source| Error::config_parse(path, source))
    }

    /// 写回 `staticsmith.toml`（可视化界面保存设置时调用）。
    pub fn save(&self, project_root: impl AsRef<Path>) -> Result<()> {
        let path = project_root.as_ref().join(CONFIG_FILE_NAME);
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw).map_err(|e| Error::io(&path, e))
    }

    /// 校验必填项，返回人类可读的问题列表（空列表表示配置合法）。
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.site.title.trim().is_empty() {
            issues.push("site.title 不能为空".to_string());
        }
        if self.build.page_size == 0 {
            issues.push("build.page_size 必须大于 0".to_string());
        }
        match self.deploy.r#type {
            DeployKind::Git if self.deploy.git.is_none() => {
                issues.push("deploy.type = \"git\" 但缺少 [deploy.git] 配置段".to_string());
            }
            DeployKind::Ftp if self.deploy.ftp.is_none() => {
                issues.push("deploy.type = \"ftp\" 但缺少 [deploy.ftp] 配置段".to_string());
            }
            _ => {}
        }
        issues
    }
}

/// 项目路径集合：把配置中的相对路径统一解析为绝对路径。
#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub root: PathBuf,
    pub content: PathBuf,
    pub templates: PathBuf,
    pub theme: PathBuf,
    pub output: PathBuf,
    /// SQLite 索引文件位置：`<root>/.staticsmith/index.db`。
    pub index_db: PathBuf,
}

impl ProjectPaths {
    pub fn new(root: impl AsRef<Path>, build: &Build) -> Self {
        let root = root.as_ref().to_path_buf();
        let join = |p: &Path| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        };
        Self {
            content: join(&build.content_dir),
            templates: join(&build.template_dir),
            theme: join(&build.theme_dir),
            output: join(&build.output_dir),
            index_db: root.join(".staticsmith").join("index.db"),
            root,
        }
    }
}

fn default_language() -> String {
    "zh-CN".to_string()
}
fn default_output_dir() -> PathBuf {
    PathBuf::from("./dist")
}
fn default_content_dir() -> PathBuf {
    PathBuf::from("./content")
}
fn default_theme_dir() -> PathBuf {
    PathBuf::from("./themes/default")
}
fn default_template_dir() -> PathBuf {
    PathBuf::from("./templates")
}
fn default_page_size() -> usize {
    10
}
fn default_branch() -> String {
    "main".to_string()
}
fn default_commit_message() -> String {
    "站点更新于 {{ now() }}".to_string()
}
fn default_auth_type() -> String {
    "token".to_string()
}
fn default_ftp_port() -> u16 {
    21
}
fn default_remote_path() -> String {
    "/public_html".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[site]
title = "我的 Rust 静态站"
description = "基于 Tauri 的可视化工具"
base_url = "https://example.com"
language = "zh-CN"

[build]
output_dir = "./dist"
content_dir = "./content"
theme_dir = "./themes/default"
template_dir = "./templates"
page_size = 10
minify = true

[deploy]
type = "git"

[deploy.git]
remote = "https://github.com/user/repo.git"
branch = "main"
commit_message = "站点更新于 {{ now() }}"
auth_type = "token"
"#;

    #[test]
    fn parses_documented_config() {
        let cfg: SiteConfig = toml::from_str(SAMPLE).unwrap();
        assert_eq!(cfg.site.title, "我的 Rust 静态站");
        assert!(cfg.build.minify);
        assert_eq!(cfg.deploy.r#type, DeployKind::Git);
        assert_eq!(cfg.deploy.git.as_ref().unwrap().branch, "main");
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn defaults_fill_missing_build_section() {
        let cfg: SiteConfig = toml::from_str("[site]\ntitle = \"t\"\n").unwrap();
        assert_eq!(cfg.build.page_size, 10);
        assert_eq!(cfg.build.output_dir, PathBuf::from("./dist"));
        assert_eq!(cfg.deploy.r#type, DeployKind::None);
    }

    #[test]
    fn validate_reports_missing_deploy_section() {
        let cfg: SiteConfig =
            toml::from_str("[site]\ntitle = \"t\"\n\n[deploy]\ntype = \"ftp\"\n").unwrap();
        assert_eq!(cfg.validate().len(), 1);
    }

    #[test]
    fn project_paths_resolve_relative_dirs() {
        let build = Build::default();
        let paths = ProjectPaths::new("/tmp/site", &build);
        assert_eq!(paths.content, PathBuf::from("/tmp/site/./content"));
        assert!(paths.index_db.ends_with("index.db"));
    }
}
