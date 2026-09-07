//! 新建项目脚手架。
//!
//! 默认模板、主题与示例内容以 `include_str!` 嵌入二进制，保证「新建站点」不依赖网络，
//! 也不会因为安装目录被移动而失效。

use std::path::{Path, PathBuf};

use crate::config::CONFIG_FILE_NAME;
use crate::error::{Error, Result};

/// (相对项目根的路径, 文件内容)
type ScaffoldFile = (&'static str, &'static str);

/// 一个新项目包含的全部文件。
pub const FILES: &[ScaffoldFile] = &[
    (
        CONFIG_FILE_NAME,
        include_str!("../scaffold/staticsmith.toml"),
    ),
    (
        "templates/layouts/base.html",
        include_str!("../scaffold/templates/layouts/base.html"),
    ),
    (
        "templates/components/header.html",
        include_str!("../scaffold/templates/components/header.html"),
    ),
    (
        "templates/components/footer.html",
        include_str!("../scaffold/templates/components/footer.html"),
    ),
    (
        "templates/components/sidebar.html",
        include_str!("../scaffold/templates/components/sidebar.html"),
    ),
    (
        "templates/components/pagination.html",
        include_str!("../scaffold/templates/components/pagination.html"),
    ),
    (
        "templates/pages/index.html",
        include_str!("../scaffold/templates/pages/index.html"),
    ),
    (
        "templates/pages/list.html",
        include_str!("../scaffold/templates/pages/list.html"),
    ),
    (
        "templates/pages/post.html",
        include_str!("../scaffold/templates/pages/post.html"),
    ),
    (
        "templates/pages/tags.html",
        include_str!("../scaffold/templates/pages/tags.html"),
    ),
    (
        "templates/pages/tag.html",
        include_str!("../scaffold/templates/pages/tag.html"),
    ),
    (
        "themes/default/static/css/main.css",
        include_str!("../scaffold/themes/default/static/css/main.css"),
    ),
    (
        "content/index.md",
        include_str!("../scaffold/content/index.md"),
    ),
    (
        "content/about.md",
        include_str!("../scaffold/content/about.md"),
    ),
    (
        "content/posts/index.md",
        include_str!("../scaffold/content/posts/index.md"),
    ),
    (
        "content/posts/hello-staticsmith.md",
        include_str!("../scaffold/content/posts/hello-staticsmith.md"),
    ),
];

/// 初始化结果。
#[derive(Debug, Clone)]
pub struct InitReport {
    pub created: Vec<PathBuf>,
    /// 已存在因而被跳过的文件。
    pub skipped: Vec<PathBuf>,
}

/// 在 `root` 下创建（或补全）项目骨架。已存在的文件不会被覆盖。
///
/// `site_title` 为 `Some` 时替换默认配置中的站点标题。
pub fn init_project(root: impl AsRef<Path>, site_title: Option<&str>) -> Result<InitReport> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|e| Error::io(root, e))?;

    let mut report = InitReport {
        created: Vec::new(),
        skipped: Vec::new(),
    };

    for (relative, contents) in FILES {
        let target = root.join(relative);
        if target.exists() {
            report.skipped.push(target);
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let body = match (site_title, *relative) {
            (Some(title), CONFIG_FILE_NAME) => contents.replacen(
                "title = \"我的 Rust 静态站\"",
                &format!("title = \"{}\"", title.replace('"', "\\\"")),
                1,
            ),
            _ => contents.to_string(),
        };
        std::fs::write(&target, body).map_err(|e| Error::io(&target, e))?;
        report.created.push(target);
    }

    Ok(report)
}

/// 目录是否已经是一个 StaticSmith 项目。
pub fn is_project(root: impl AsRef<Path>) -> bool {
    root.as_ref().join(CONFIG_FILE_NAME).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_creates_full_skeleton() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_project(dir.path(), Some("测试站点")).unwrap();

        assert_eq!(report.created.len(), FILES.len());
        assert!(report.skipped.is_empty());
        assert!(is_project(dir.path()));
        assert!(dir.path().join("templates/components/header.html").exists());

        let config = std::fs::read_to_string(dir.path().join(CONFIG_FILE_NAME)).unwrap();
        assert!(config.contains("title = \"测试站点\""));
    }

    #[test]
    fn init_is_idempotent_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path(), None).unwrap();
        std::fs::write(dir.path().join("content/index.md"), "手工内容").unwrap();

        let second = init_project(dir.path(), None).unwrap();
        assert!(second.created.is_empty());
        assert_eq!(second.skipped.len(), FILES.len());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("content/index.md")).unwrap(),
            "手工内容"
        );
    }
}
