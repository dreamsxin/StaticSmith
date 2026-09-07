//! 产物清单。
//!
//! 界面此前只能看到 `content/` 下的 Markdown，而构建实际会产出一批「非内容页」：
//! 标签总览、单标签页、分页页、`sitemap.xml`、`feed.xml`、静态资源。
//! 它们在界面里没有任何入口，用户改完标签也无从确认结果。
//!
//! 这里扫描输出目录并分类，让「生成了什么」变成可浏览的列表。

use std::path::Path;

use serde::Serialize;

use crate::config::Taxonomy as TaxonomyConfig;
use crate::error::{Error, Result};
use crate::util;

/// 产物类型。界面按它分组展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputKind {
    /// 普通页面（来自内容文件）
    Page,
    /// 列表页的第 2 页及以后
    Pagination,
    /// 标签总览与单标签页
    Taxonomy,
    Sitemap,
    Feed,
    /// 复制过去的静态资源
    Asset,
}

impl OutputKind {
    /// 分组标题，前端直接用。
    pub fn label(self) -> &'static str {
        match self {
            OutputKind::Page => "页面",
            OutputKind::Pagination => "分页",
            OutputKind::Taxonomy => "标签页",
            OutputKind::Sitemap => "站点地图",
            OutputKind::Feed => "订阅",
            OutputKind::Asset => "静态资源",
        }
    }
}

/// 一个产物文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputFile {
    /// 相对输出目录的正斜杠路径，如 `tags/模板/index.html`。
    pub path: String,
    /// 站内地址，如 `/tags/模板/`。
    pub url: String,
    pub kind: OutputKind,
    pub size: u64,
}

/// 扫描输出目录。目录不存在（还没生成过）时返回空列表，而不是报错。
pub fn scan(output_dir: &Path, taxonomy: &TaxonomyConfig) -> Result<Vec<OutputFile>> {
    if !output_dir.is_dir() {
        return Ok(Vec::new());
    }
    let taxonomy_prefix = taxonomy.enabled.then(|| taxonomy.normalized_slug());

    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(output_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(output_dir)
            .unwrap_or(entry.path());
        let path = util::to_slash(relative);
        let size = entry
            .metadata()
            .map_err(|e| Error::io(entry.path(), std::io::Error::other(e.to_string())))?
            .len();

        files.push(OutputFile {
            kind: classify(&path, taxonomy_prefix.as_deref()),
            url: url_for(&path),
            path,
            size,
        });
    }

    // 先按类型（页面在前），同类型按路径，方便界面直接渲染
    files.sort_by(|a, b| order(a.kind).cmp(&order(b.kind)).then(a.path.cmp(&b.path)));
    Ok(files)
}

fn order(kind: OutputKind) -> u8 {
    match kind {
        OutputKind::Page => 0,
        OutputKind::Taxonomy => 1,
        OutputKind::Pagination => 2,
        OutputKind::Sitemap => 3,
        OutputKind::Feed => 4,
        OutputKind::Asset => 5,
    }
}

/// 按路径判断产物类型。
///
/// `taxonomy_prefix` 为 `None` 表示标签功能关闭，此时 `tags/` 目录下的遗留文件
/// 按普通页面对待——它可能是上一次开启时留下的，不该假装还是标签页。
pub fn classify(path: &str, taxonomy_prefix: Option<&str>) -> OutputKind {
    if path == "sitemap.xml" {
        return OutputKind::Sitemap;
    }
    if path == "feed.xml" {
        return OutputKind::Feed;
    }
    if !path.ends_with(".html") {
        return OutputKind::Asset;
    }
    if let Some(prefix) = taxonomy_prefix {
        if !prefix.is_empty() && path.starts_with(&format!("{prefix}/")) {
            return OutputKind::Taxonomy;
        }
    }
    if is_pagination(path) {
        return OutputKind::Pagination;
    }
    OutputKind::Page
}

/// `posts/page/2/index.html` 这类分页产物。
fn is_pagination(path: &str) -> bool {
    let mut segments = path.split('/').rev();
    // 结尾形如 …/page/<数字>/index.html
    segments.next() == Some("index.html")
        && segments
            .next()
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        && segments.next() == Some("page")
}

/// 产物路径 → 站内地址。`index.html` 折叠成目录形式。
pub fn url_for(path: &str) -> String {
    match path.strip_suffix("index.html") {
        Some(dir) => format!("/{dir}"),
        None => format!("/{path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taxonomy() -> TaxonomyConfig {
        TaxonomyConfig::default()
    }

    #[test]
    fn classifies_pages_and_pagination() {
        assert_eq!(classify("index.html", Some("tags")), OutputKind::Page);
        assert_eq!(
            classify("posts/hello/index.html", Some("tags")),
            OutputKind::Page
        );
        assert_eq!(
            classify("posts/page/2/index.html", Some("tags")),
            OutputKind::Pagination
        );
        assert_eq!(
            classify("page/2/index.html", Some("tags")),
            OutputKind::Pagination
        );
    }

    #[test]
    fn classifies_taxonomy_by_configured_prefix() {
        assert_eq!(
            classify("tags/index.html", Some("tags")),
            OutputKind::Taxonomy
        );
        assert_eq!(
            classify("tags/模板/index.html", Some("tags")),
            OutputKind::Taxonomy
        );
        // 标签页的分页仍归类为标签页，避免用户在两个分组里找同一批东西
        assert_eq!(
            classify("tags/模板/page/2/index.html", Some("tags")),
            OutputKind::Taxonomy
        );
        // 自定义前缀
        assert_eq!(
            classify("topics/rust/index.html", Some("topics")),
            OutputKind::Taxonomy
        );
    }

    #[test]
    fn disabled_taxonomy_leaves_stale_files_as_pages() {
        assert_eq!(classify("tags/index.html", None), OutputKind::Page);
    }

    #[test]
    fn classifies_xml_and_assets() {
        assert_eq!(classify("sitemap.xml", Some("tags")), OutputKind::Sitemap);
        assert_eq!(classify("feed.xml", Some("tags")), OutputKind::Feed);
        assert_eq!(classify("css/main.css", Some("tags")), OutputKind::Asset);
        assert_eq!(
            classify("images/ab/cd.png", Some("tags")),
            OutputKind::Asset
        );
    }

    #[test]
    fn pagination_detection_ignores_lookalikes() {
        assert_eq!(classify("page/index.html", Some("tags")), OutputKind::Page);
        assert_eq!(
            classify("posts/page/abc/index.html", Some("tags")),
            OutputKind::Page
        );
        assert_eq!(
            classify("posts/2/index.html", Some("tags")),
            OutputKind::Page
        );
    }

    #[test]
    fn urls_fold_index_html_into_directories() {
        assert_eq!(url_for("index.html"), "/");
        assert_eq!(url_for("posts/hello/index.html"), "/posts/hello/");
        assert_eq!(url_for("sitemap.xml"), "/sitemap.xml");
        assert_eq!(url_for("css/main.css"), "/css/main.css");
    }

    #[test]
    fn scan_returns_empty_when_nothing_was_built() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("dist");
        assert!(scan(&missing, &taxonomy()).unwrap().is_empty());
    }

    #[test]
    fn scan_groups_pages_before_assets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("tags/模板")).unwrap();
        std::fs::create_dir_all(root.join("css")).unwrap();
        std::fs::write(root.join("index.html"), "home").unwrap();
        std::fs::write(root.join("tags/index.html"), "tags").unwrap();
        std::fs::write(root.join("tags/模板/index.html"), "term").unwrap();
        std::fs::write(root.join("sitemap.xml"), "<urlset/>").unwrap();
        std::fs::write(root.join("css/main.css"), "body{}").unwrap();

        let files = scan(root, &taxonomy()).unwrap();
        let kinds: Vec<OutputKind> = files.iter().map(|f| f.kind).collect();
        assert_eq!(
            kinds,
            vec![
                OutputKind::Page,
                OutputKind::Taxonomy,
                OutputKind::Taxonomy,
                OutputKind::Sitemap,
                OutputKind::Asset
            ]
        );

        let home = &files[0];
        assert_eq!(home.url, "/");
        assert_eq!(home.size, 4);
    }

    #[test]
    fn kind_labels_are_human_readable() {
        assert_eq!(OutputKind::Taxonomy.label(), "标签页");
        assert_eq!(OutputKind::Pagination.label(), "分页");
    }
}
