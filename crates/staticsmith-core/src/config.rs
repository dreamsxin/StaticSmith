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
    pub assets: Assets,
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
    /// 站点级静态资源目录，整体复制到输出目录根部。
    #[serde(default = "default_static_dir")]
    pub static_dir: PathBuf,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    #[serde(default)]
    pub minify: bool,
}

/// 编辑器插入的图片等媒体资源如何落盘。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assets {
    /// 相对 `build.static_dir` 的子目录。
    ///
    /// 刻意限定为相对路径：资源目录必须位于 static_dir 之内，
    /// 这样「复制到产物」与「页面里的 URL」由同一份路径推导出来，不可能对不上。
    #[serde(default = "default_assets_dir")]
    pub dir: String,
    /// 文件命名策略。
    #[serde(default)]
    pub naming: AssetNaming,
    /// 哈希截断长度（十六进制字符数），8..=64。
    #[serde(default = "default_hash_length")]
    pub hash_length: usize,
    /// 是否用哈希前两位做二级目录，避免单目录堆积上万文件。
    #[serde(default = "default_true")]
    pub shard: bool,
    /// 单个文件大小上限（MB），0 表示不限制。
    #[serde(default = "default_max_asset_mb")]
    pub max_size_mb: u64,
    /// URL 前缀覆盖。留空时按 `dir` 推导，例如 `images` → `/images/`。
    #[serde(default)]
    pub url_prefix: Option<String>,
}

/// 资源文件名的生成方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetNaming {
    /// SHA-256 内容哈希（默认）。同一张图重复粘贴只保存一份。
    #[default]
    Sha256,
    /// MD5 内容哈希。仅用于与既有站点资源命名保持一致，不用于安全场景。
    Md5,
    /// 保留原文件名（清洗后）；重名且内容不同时追加短哈希。
    Original,
}

impl Default for Assets {
    fn default() -> Self {
        Self {
            dir: default_assets_dir(),
            naming: AssetNaming::default(),
            hash_length: default_hash_length(),
            shard: true,
            max_size_mb: default_max_asset_mb(),
            url_prefix: None,
        }
    }
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
            static_dir: default_static_dir(),
            page_size: default_page_size(),
            minify: false,
        }
    }
}

impl Assets {
    /// 资源目录的规范化形式：去掉首尾斜杠，统一正斜杠。
    pub fn normalized_dir(&self) -> String {
        self.dir.replace('\\', "/").trim_matches('/').to_string()
    }

    /// 站内 URL 前缀，形如 `/images/`。
    pub fn url_prefix(&self) -> String {
        if let Some(prefix) = &self.url_prefix {
            let trimmed = prefix.trim_end_matches('/');
            return format!("{}/", if trimmed.is_empty() { "" } else { trimmed });
        }
        let dir = self.normalized_dir();
        if dir.is_empty() {
            "/".to_string()
        } else {
            format!("/{dir}/")
        }
    }

    /// 哈希截断长度，夹在 8..=64 之间避免配置写错导致文件名碰撞或过长。
    pub fn effective_hash_length(&self) -> usize {
        self.hash_length.clamp(8, 64)
    }

    fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let dir = self.normalized_dir();
        if dir.split('/').any(|s| s == "..") {
            issues.push("assets.dir 不能包含 `..`".to_string());
        }
        if Path::new(&self.dir).is_absolute() {
            issues.push("assets.dir 必须是相对 build.static_dir 的路径".to_string());
        }
        if !(8..=64).contains(&self.hash_length) {
            issues.push("assets.hash_length 必须在 8..=64 之间".to_string());
        }
        issues
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
        issues.extend(self.assets.validate());
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
    /// 站点级静态资源目录。
    pub static_dir: PathBuf,
    /// 编辑器插入的媒体资源目录（位于 `static_dir` 之内）。
    pub assets: PathBuf,
    /// SQLite 索引文件位置：`<root>/.staticsmith/index.db`。
    pub index_db: PathBuf,
}

impl ProjectPaths {
    pub fn new(root: impl AsRef<Path>, build: &Build, assets: &Assets) -> Self {
        let root = root.as_ref().to_path_buf();
        let join = |p: &Path| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            }
        };
        let static_dir = join(&build.static_dir);
        let assets_dir = assets
            .normalized_dir()
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(static_dir.clone(), |acc, segment| acc.join(segment));
        Self {
            content: join(&build.content_dir),
            templates: join(&build.template_dir),
            theme: join(&build.theme_dir),
            output: join(&build.output_dir),
            assets: assets_dir,
            static_dir,
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
fn default_static_dir() -> PathBuf {
    PathBuf::from("./static")
}
fn default_assets_dir() -> String {
    "images".to_string()
}
fn default_hash_length() -> usize {
    16
}
fn default_max_asset_mb() -> u64 {
    32
}
fn default_true() -> bool {
    true
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
        let assets = Assets::default();
        let paths = ProjectPaths::new("/tmp/site", &build, &assets);
        assert_eq!(paths.content, PathBuf::from("/tmp/site/./content"));
        assert!(paths.index_db.ends_with("index.db"));
        assert!(paths.assets.ends_with("images"));
        assert!(paths.assets.starts_with(&paths.static_dir));
    }

    #[test]
    fn assets_defaults_derive_url_prefix_from_dir() {
        let assets = Assets::default();
        assert_eq!(assets.dir, "images");
        assert_eq!(assets.url_prefix(), "/images/");
        assert_eq!(assets.naming, AssetNaming::Sha256);
        assert!(assets.shard);
        assert!(assets.validate().is_empty());
    }

    #[test]
    fn assets_url_prefix_override_is_normalized() {
        let assets = Assets {
            dir: "media/img".into(),
            url_prefix: Some("/cdn/assets".into()),
            ..Assets::default()
        };
        assert_eq!(assets.url_prefix(), "/cdn/assets/");
        assert_eq!(assets.normalized_dir(), "media/img");
    }

    #[test]
    fn assets_reject_traversal_and_bad_hash_length() {
        let assets = Assets {
            dir: "../outside".into(),
            hash_length: 4,
            ..Assets::default()
        };
        let issues = assets.validate();
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(assets.effective_hash_length(), 8, "越界值应被夹紧");
    }

    #[test]
    fn nested_assets_dir_maps_to_nested_path_and_url() {
        let assets = Assets {
            dir: "media/2026".into(),
            ..Assets::default()
        };
        let paths = ProjectPaths::new("/tmp/site", &Build::default(), &assets);
        assert!(paths.assets.ends_with(Path::new("media").join("2026")));
        assert_eq!(assets.url_prefix(), "/media/2026/");
    }
}
