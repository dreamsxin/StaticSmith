//! 新建项目脚手架。
//!
//! 默认模板、主题与示例内容以 `include_str!` 嵌入二进制，保证「新建站点」不依赖网络，
//! 也不会因为安装目录被移动而失效。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::CONFIG_FILE_NAME;
use crate::error::{Error, Result};

/// (相对项目根的路径, 文件内容)
type ScaffoldFile = (&'static str, &'static str);

/// 站点模板预设：决定 `templates/` 与 `themes/` 里放哪一套。
///
/// **配置与样例内容两套预设共用**，区别只在版式与观感。多一套内容就多一份要同步
/// 维护的文案，而那几篇样例（讲级联更新、讲关于页）放在文档站与博客里都读得通。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    /// 极简文档站：单栏、Rust 橙、内容居中限宽。适合项目文档与说明页。
    #[default]
    Docs,
    /// 博客园风格：定宽居中、左文右栏、随笔列表 + 侧栏分类。适合个人技术博客。
    Blog,
}

impl Preset {
    /// 全部预设，顺序即界面上的展示顺序（第一个是默认值）。
    pub const ALL: [Preset; 2] = [Preset::Docs, Preset::Blog];

    /// 配置与命令行里用的标识。
    pub fn slug(self) -> &'static str {
        match self {
            Preset::Docs => "docs",
            Preset::Blog => "blog",
        }
    }

    /// 界面上的名字。
    pub fn title(self) -> &'static str {
        match self {
            Preset::Docs => "极简文档站",
            Preset::Blog => "博客园风格博客",
        }
    }

    /// 界面上那一句说明——用户要靠它选，所以说的是「长什么样、适合谁」，
    /// 而不是「用了哪些模板」。
    pub fn description(self) -> &'static str {
        match self {
            Preset::Docs => "单栏、留白多、Rust 橙点缀。写项目文档、说明页、作品集合适。",
            Preset::Blog => "定宽居中、左文右栏、随笔列表配侧栏分类。写个人技术博客合适。",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.slug() == slug.trim().to_ascii_lowercase())
    }

    fn own_files(self) -> &'static [ScaffoldFile] {
        match self {
            Preset::Docs => DOCS_FILES,
            Preset::Blog => BLOG_FILES,
        }
    }
}

/// 两套预设共用：配置与样例内容。
const SHARED_FILES: &[ScaffoldFile] = &[
    (
        CONFIG_FILE_NAME,
        include_str!("../scaffold/staticsmith.toml"),
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

const DOCS_FILES: &[ScaffoldFile] = &[
    (
        "templates/layouts/base.html",
        include_str!("../scaffold/presets/docs/templates/layouts/base.html"),
    ),
    (
        "templates/components/header.html",
        include_str!("../scaffold/presets/docs/templates/components/header.html"),
    ),
    (
        "templates/components/footer.html",
        include_str!("../scaffold/presets/docs/templates/components/footer.html"),
    ),
    (
        "templates/components/sidebar.html",
        include_str!("../scaffold/presets/docs/templates/components/sidebar.html"),
    ),
    (
        "templates/components/pagination.html",
        include_str!("../scaffold/presets/docs/templates/components/pagination.html"),
    ),
    (
        "templates/pages/index.html",
        include_str!("../scaffold/presets/docs/templates/pages/index.html"),
    ),
    (
        "templates/pages/list.html",
        include_str!("../scaffold/presets/docs/templates/pages/list.html"),
    ),
    (
        "templates/pages/post.html",
        include_str!("../scaffold/presets/docs/templates/pages/post.html"),
    ),
    (
        "templates/pages/tags.html",
        include_str!("../scaffold/presets/docs/templates/pages/tags.html"),
    ),
    (
        "templates/pages/tag.html",
        include_str!("../scaffold/presets/docs/templates/pages/tag.html"),
    ),
    (
        "themes/default/static/css/main.css",
        include_str!("../scaffold/presets/docs/themes/default/static/css/main.css"),
    ),
];

const BLOG_FILES: &[ScaffoldFile] = &[
    (
        "templates/layouts/base.html",
        include_str!("../scaffold/presets/blog/templates/layouts/base.html"),
    ),
    (
        "templates/components/header.html",
        include_str!("../scaffold/presets/blog/templates/components/header.html"),
    ),
    (
        "templates/components/footer.html",
        include_str!("../scaffold/presets/blog/templates/components/footer.html"),
    ),
    (
        "templates/components/sidebar.html",
        include_str!("../scaffold/presets/blog/templates/components/sidebar.html"),
    ),
    (
        "templates/components/pagination.html",
        include_str!("../scaffold/presets/blog/templates/components/pagination.html"),
    ),
    (
        "templates/pages/index.html",
        include_str!("../scaffold/presets/blog/templates/pages/index.html"),
    ),
    (
        "templates/pages/list.html",
        include_str!("../scaffold/presets/blog/templates/pages/list.html"),
    ),
    (
        "templates/pages/post.html",
        include_str!("../scaffold/presets/blog/templates/pages/post.html"),
    ),
    (
        "templates/pages/tags.html",
        include_str!("../scaffold/presets/blog/templates/pages/tags.html"),
    ),
    (
        "templates/pages/tag.html",
        include_str!("../scaffold/presets/blog/templates/pages/tag.html"),
    ),
    (
        "themes/default/static/css/main.css",
        include_str!("../scaffold/presets/blog/themes/default/static/css/main.css"),
    ),
];

/// 某个预设会创建的全部文件（共用部分 + 该预设自己的模板与主题）。
pub fn files(preset: Preset) -> Vec<ScaffoldFile> {
    SHARED_FILES
        .iter()
        .chain(preset.own_files())
        .copied()
        .collect()
}

/// 初始化结果。
#[derive(Debug, Clone)]
pub struct InitReport {
    pub created: Vec<PathBuf>,
    /// 已存在因而被跳过的文件。
    pub skipped: Vec<PathBuf>,
    /// 用的哪套预设，界面据此提示「装的是哪一套」。
    pub preset: Preset,
}

/// 在 `root` 下创建（或补全）项目骨架。已存在的文件不会被覆盖。
///
/// `site_title` 为 `Some` 时替换默认配置中的站点标题。
///
/// **不覆盖已存在的文件，所以对着已有项目换 `preset` 是不会换皮的**——那属于「换外观」，
/// 走主题包（`theme::import`）而不是脚手架。脚手架只负责从零到有。
pub fn init_project(
    root: impl AsRef<Path>,
    site_title: Option<&str>,
    preset: Preset,
) -> Result<InitReport> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|e| Error::io(root, e))?;

    let mut report = InitReport {
        created: Vec::new(),
        skipped: Vec::new(),
        preset,
    };

    for (relative, contents) in files(preset) {
        let target = root.join(relative);
        if target.exists() {
            report.skipped.push(target);
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let body = match (site_title, relative) {
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
        let report = init_project(dir.path(), Some("测试站点"), Preset::Docs).unwrap();

        assert_eq!(report.created.len(), files(Preset::Docs).len());
        assert!(report.skipped.is_empty());
        assert!(is_project(dir.path()));
        assert!(dir.path().join("templates/components/header.html").exists());

        let config = std::fs::read_to_string(dir.path().join(CONFIG_FILE_NAME)).unwrap();
        assert!(config.contains("title = \"测试站点\""));
    }

    #[test]
    fn init_is_idempotent_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path(), None, Preset::Docs).unwrap();
        std::fs::write(dir.path().join("content/index.md"), "手工内容").unwrap();

        let second = init_project(dir.path(), None, Preset::Docs).unwrap();
        assert!(second.created.is_empty());
        assert_eq!(second.skipped.len(), files(Preset::Docs).len());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("content/index.md")).unwrap(),
            "手工内容"
        );
    }

    /// 两套预设写出**同一组路径**，只是内容不同。
    ///
    /// 这条不变量很重要：路径一致意味着换预设不影响 `docs/templates.md` 里那套目录约定，
    /// 也意味着模板名推导（`pages/post.html` 之类）对两套都成立。
    #[test]
    fn presets_write_the_same_paths() {
        let docs: Vec<&str> = files(Preset::Docs).iter().map(|(p, _)| *p).collect();
        let blog: Vec<&str> = files(Preset::Blog).iter().map(|(p, _)| *p).collect();
        assert_eq!(docs, blog);
    }

    #[test]
    fn blog_preset_writes_its_own_look() {
        let dir = tempfile::tempdir().unwrap();
        init_project(dir.path(), Some("我的博客"), Preset::Blog).unwrap();

        let base = std::fs::read_to_string(dir.path().join("templates/layouts/base.html")).unwrap();
        assert!(base.contains("id=\"mainContent\""), "应当是博客园那套骨架");
        let css =
            std::fs::read_to_string(dir.path().join("themes/default/static/css/main.css")).unwrap();
        assert!(
            css.contains("prefers-color-scheme: dark"),
            "博客预设带深色模式"
        );
    }

    #[test]
    fn preset_slugs_round_trip() {
        for preset in Preset::ALL {
            assert_eq!(Preset::from_slug(preset.slug()), Some(preset));
            assert_eq!(
                Preset::from_slug(&preset.slug().to_uppercase()),
                Some(preset)
            );
        }
        assert_eq!(Preset::from_slug("没有这套"), None);
    }

    #[test]
    fn default_preset_is_docs() {
        assert_eq!(Preset::default(), Preset::Docs);
        assert_eq!(Preset::ALL[0], Preset::default());
    }
}
