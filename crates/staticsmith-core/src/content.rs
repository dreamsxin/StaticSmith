use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::util;

/// front matter 围栏标记（Zola 风格 TOML）。
const FENCE: &str = "+++";

/// 内容文件的 front matter。未声明的字段进入 `extra`，模板中通过 `page.extra.xxx` 访问。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FrontMatter {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub date: Option<String>,
    /// 使用的页面模板（相对 `templates/` 的路径），缺省由目录约定推导。
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub draft: bool,
    /// 列表页排序权重，数值越小越靠前（同 `date` 降序互补）。
    #[serde(default)]
    pub weight: i64,
    #[serde(default)]
    pub extra: toml::Table,
}

/// 一篇已解析的内容页。
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    /// 相对 `content/` 的源文件路径，如 `posts/hello.md`。索引与依赖图的主键。
    pub source: String,
    /// 输出路径，如 `posts/hello/index.html`。
    pub output: String,
    /// 站内链接，如 `/posts/hello/`。
    pub url: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub date: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub draft: bool,
    pub weight: i64,
    /// 所在栏目（相对 content 的目录，根目录为空串）。
    pub section: String,
    /// 是否是栏目索引页（`index.md` / `_index.md`），列表分页以它为容器。
    pub is_index: bool,
    /// 页面模板名（Tera 中注册的名字），如 `pages/post.html`。
    pub template: String,
    /// 渲染后的正文 HTML。
    pub content: String,
    /// 源文件内容哈希，用于增量构建判定。
    pub hash: String,
    pub extra: toml::Table,
}

impl Page {
    /// 解析单个 Markdown 文件。
    ///
    /// `content_root` 用于计算相对路径；`source_path` 必须位于其下。
    pub fn from_file(content_root: &Path, source_path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(source_path).map_err(|e| Error::io(source_path, e))?;
        Self::from_str(content_root, source_path, &raw)
    }

    /// 从原始文本解析（便于测试与内存中的编辑器预览）。
    pub fn from_str(content_root: &Path, source_path: &Path, raw: &str) -> Result<Self> {
        let (fm, body) = split_front_matter(source_path, raw)?;

        let rel = source_path
            .strip_prefix(content_root)
            .unwrap_or(source_path)
            .to_path_buf();
        let source = util::to_slash(&rel);

        let file_stem = rel
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "index".to_string());
        let slug = fm
            .slug
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| slug::slugify(&file_stem));

        let dir = util::to_slash(rel.parent().unwrap_or_else(|| Path::new("")));
        let is_index = file_stem == "index" || file_stem == "_index";
        let (output, url) = output_paths(&dir, &file_stem, &slug);
        let template = fm
            .template
            .clone()
            .unwrap_or_else(|| default_template(&dir, &file_stem));

        let title = if fm.title.trim().is_empty() {
            first_heading(&body).unwrap_or_else(|| file_stem.clone())
        } else {
            fm.title.clone()
        };

        Ok(Self {
            source,
            output,
            url,
            slug,
            title,
            description: fm.description.clone(),
            date: fm.date.as_deref().and_then(parse_date),
            tags: fm.tags.clone(),
            draft: fm.draft,
            weight: fm.weight,
            section: dir,
            is_index,
            template,
            content: markdown_to_html(&body),
            hash: util::hash_str(raw),
            extra: fm.extra,
        })
    }

    /// 是否参与构建输出（草稿在生产构建中跳过）。
    pub fn is_publishable(&self) -> bool {
        !self.draft
    }
}

/// 递归扫描 `content/` 下所有 `.md` / `.markdown` 文件并解析。
pub fn load_all(content_root: &Path) -> Result<Vec<Page>> {
    if !content_root.exists() {
        return Err(Error::InvalidProject(format!(
            "内容目录不存在: {}",
            content_root.display()
        )));
    }
    let mut pages = Vec::new();
    for entry in walkdir::WalkDir::new(content_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let is_markdown = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("markdown")
        );
        if is_markdown {
            pages.push(Page::from_file(content_root, path)?);
        }
    }
    pages.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(pages)
}

/// 拆分 front matter 与正文。没有围栏时整体视为正文。
fn split_front_matter(path: &Path, raw: &str) -> Result<(FrontMatter, String)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix(FENCE) else {
        return Ok((FrontMatter::default(), trimmed.to_string()));
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let Some(end) = rest.find(FENCE) else {
        return Err(Error::FrontMatter {
            path: path.to_path_buf(),
            message: "front matter 缺少结束的 `+++`".to_string(),
        });
    };
    let (fm_raw, body) = rest.split_at(end);
    let fm: FrontMatter = toml::from_str(fm_raw).map_err(|e| Error::FrontMatter {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let body = body[FENCE.len()..]
        .trim_start_matches(['\r', '\n'])
        .to_string();
    Ok((fm, body))
}

/// 目录约定：`content/index.md` → `pages/index.html`，其余 → `pages/post.html`。
fn default_template(dir: &str, file_stem: &str) -> String {
    if dir.is_empty() && (file_stem == "index" || file_stem == "_index") {
        "pages/index.html".to_string()
    } else if file_stem == "index" || file_stem == "_index" {
        "pages/list.html".to_string()
    } else {
        "pages/post.html".to_string()
    }
}

/// 生成 pretty URL：`posts/hello.md` → (`posts/hello/index.html`, `/posts/hello/`)。
fn output_paths(dir: &str, file_stem: &str, slug: &str) -> (String, String) {
    let is_index = file_stem == "index" || file_stem == "_index";
    let segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let mut parts = segments.clone();
    if !is_index {
        parts.push(slug);
    }
    let joined = parts.join("/");
    let output = if joined.is_empty() {
        "index.html".to_string()
    } else {
        format!("{joined}/index.html")
    };
    let url = if joined.is_empty() {
        "/".to_string()
    } else {
        format!("/{joined}/")
    };
    (output, url)
}

/// 支持 `2026-09-07` 与 RFC3339 两种写法。
fn parse_date(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

/// 兜底标题：取正文第一个 `# ` 标题。
fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l[2..].trim().to_string())
}

/// Markdown → HTML（启用表格、脚注、任务列表、删除线）。
pub fn markdown_to_html(markdown: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    let parser = Parser::new_ext(markdown, options);
    let mut out = String::with_capacity(markdown.len() * 3 / 2);
    html::push_html(&mut out, parser);
    out
}

/// 内容目录相对路径 → 绝对路径（供 Tauri 命令层保存文件用）。
pub fn resolve_source(content_root: &Path, source: &str) -> PathBuf {
    content_root.join(source.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str, raw: &str) -> Page {
        Page::from_str(
            Path::new("/site/content"),
            &Path::new("/site/content").join(path),
            raw,
        )
        .unwrap()
    }

    #[test]
    fn parses_front_matter_and_body() {
        let page = parse(
            "posts/hello.md",
            "+++\ntitle = \"你好\"\ndate = \"2026-09-07\"\ntags = [\"rust\"]\n+++\n\n正文**加粗**\n",
        );
        assert_eq!(page.title, "你好");
        assert_eq!(page.tags, vec!["rust"]);
        assert_eq!(page.source, "posts/hello.md");
        assert_eq!(page.output, "posts/hello/index.html");
        assert_eq!(page.url, "/posts/hello/");
        assert_eq!(page.template, "pages/post.html");
        assert!(page.content.contains("<strong>"));
        assert!(page.date.is_some());
    }

    #[test]
    fn root_index_uses_index_template() {
        let page = parse("index.md", "+++\ntitle = \"首页\"\n+++\n内容\n");
        assert_eq!(page.output, "index.html");
        assert_eq!(page.url, "/");
        assert_eq!(page.template, "pages/index.html");
    }

    #[test]
    fn section_index_uses_list_template() {
        let page = parse("posts/index.md", "+++\ntitle = \"归档\"\n+++\n");
        assert_eq!(page.output, "posts/index.html");
        assert_eq!(page.template, "pages/list.html");
    }

    #[test]
    fn missing_front_matter_is_allowed() {
        let page = parse("posts/plain.md", "# 只有标题\n\n段落\n");
        assert_eq!(page.title, "只有标题");
        assert!(page.content.contains("<h1>"));
    }

    #[test]
    fn unterminated_front_matter_is_an_error() {
        let err = Page::from_str(
            Path::new("/site/content"),
            Path::new("/site/content/bad.md"),
            "+++\ntitle = \"x\"\n",
        )
        .unwrap_err();
        assert!(matches!(err, Error::FrontMatter { .. }));
    }

    #[test]
    fn explicit_slug_overrides_file_stem() {
        let page = parse("posts/a.md", "+++\nslug = \"custom-url\"\n+++\n");
        assert_eq!(page.url, "/posts/custom-url/");
    }

    #[test]
    fn hash_changes_with_content() {
        let a = parse("a.md", "+++\ntitle=\"a\"\n+++\n1");
        let b = parse("a.md", "+++\ntitle=\"a\"\n+++\n2");
        assert_ne!(a.hash, b.hash);
    }
}
