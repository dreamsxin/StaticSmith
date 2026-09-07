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
    /// SEO 关键词。留空时回退到 `tags`——运营上两者往往是同一批词，
    /// 但仍分开存放：标签是站内导航（会生成标签页），关键词只进 meta。
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub draft: bool,
    /// 列表页排序权重，数值越小越靠前（同 `date` 降序互补）。
    #[serde(default)]
    pub weight: i64,
    #[serde(default)]
    pub extra: toml::Table,
    /// 未声明的顶层键。`categories` 这类自定义分类维度从这里读出来，
    /// 不必为每加一个维度就改一次结构体。
    #[serde(flatten)]
    pub rest: toml::Table,
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
    /// SEO 关键词，未写时等于 `tags`。
    pub keywords: Vec<String>,
    /// 各分类维度的词条：`tags`、`categories` 或任意自定义字段。
    pub taxonomies: std::collections::BTreeMap<String, Vec<String>>,
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

        // 关键词没写就用标签：运营上多数文章两者一致，逼用户写两遍只会漏。
        let keywords = if fm.keywords.is_empty() {
            fm.tags.clone()
        } else {
            fm.keywords.clone()
        };

        let taxonomies = collect_taxonomy_fields(&fm);

        Ok(Self {
            source,
            output,
            url,
            slug,
            title,
            description: fm.description.clone(),
            date: fm.date.as_deref().and_then(parse_date),
            tags: fm.tags.clone(),
            keywords,
            taxonomies,
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

    /// 发布时间是否已到。
    ///
    /// `publish_future` 为 `true`（默认）时永远为真；为 `false` 时，`date` 晚于 `now`
    /// 的文章算「排着队」，暂不进产物。没写日期的一律算已到——把「忘了写日期」
    /// 当成「永不发布」只会让人莫名其妙地少一篇。
    pub fn is_released_at(&self, now: DateTime<Utc>, publish_future: bool) -> bool {
        if publish_future {
            return true;
        }
        self.date.map(|date| date <= now).unwrap_or(true)
    }
}

/// 从 front matter 里挑出各分类维度的词条。
///
/// `tags` 是声明字段，其余（`categories` 等）来自未声明的顶层键——
/// 只认「字符串数组」，把 `draft = true` 这类标量挡在外面。
fn collect_taxonomy_fields(fm: &FrontMatter) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = std::collections::BTreeMap::new();
    let clean = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let tags = clean(&fm.tags);
    if !tags.is_empty() {
        out.insert("tags".to_string(), tags);
    }

    for (key, value) in &fm.rest {
        let Some(array) = value.as_array() else {
            continue;
        };
        let items: Vec<String> = array
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            out.insert(key.clone(), items);
        }
    }
    out
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

/// 新建内容的请求参数。
///
/// 界面上「新建文章」只需要填标题与栏目，其余字段给默认值即可。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewContent {
    /// 栏目（相对 `content/` 的目录），根目录传空串。
    #[serde(default)]
    pub section: String,
    pub title: String,
    /// 文件名主干，缺省由标题推导。
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 默认建为草稿，避免刚写一行就被发布出去。
    #[serde(default = "default_draft")]
    pub draft: bool,
}

fn default_draft() -> bool {
    true
}

impl NewContent {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            draft: true,
            ..Default::default()
        }
    }

    pub fn in_section(mut self, section: impl Into<String>) -> Self {
        self.section = section.into();
        self
    }

    /// 目标源文件路径（相对 `content/`）。栏目里的路径穿越片段会被丢弃。
    pub fn source_path(&self) -> String {
        let section = util::sanitize_relative_dir(&self.section);
        let stem = self
            .slug
            .as_deref()
            .map(util::slugify_name)
            .filter(|s| !s.is_empty())
            .or_else(|| Some(util::slugify_name(&self.title)))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "untitled".to_string());
        if section.is_empty() {
            format!("{stem}.md")
        } else {
            format!("{section}/{stem}.md")
        }
    }

    /// 生成带 front matter 的骨架文件内容。`date` 为 `YYYY-MM-DD`。
    pub fn to_markdown(&self, date: &str) -> String {
        let mut fm = String::from("+++\n");
        fm.push_str(&format!("title = {}\n", toml_string(&self.title)));
        fm.push_str(&format!("date = {}\n", toml_string(date)));
        if !self.description.is_empty() {
            fm.push_str(&format!(
                "description = {}\n",
                toml_string(&self.description)
            ));
        }
        if let Some(template) = &self.template {
            fm.push_str(&format!("template = {}\n", toml_string(template)));
        }
        if !self.tags.is_empty() {
            let tags: Vec<String> = self.tags.iter().map(|t| toml_string(t)).collect();
            fm.push_str(&format!("tags = [{}]\n", tags.join(", ")));
        }
        if self.draft {
            fm.push_str("draft = true\n");
        }
        fm.push_str("+++\n\n");
        fm
    }
}

/// TOML 字符串字面量：转义反斜杠与双引号即可，标题里不会有控制字符。
fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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

    #[test]
    fn new_content_derives_path_from_title() {
        let request = NewContent::new("统一模板 与 级联更新").in_section("posts");
        assert_eq!(request.source_path(), "posts/统一模板-与-级联更新.md");
    }

    #[test]
    fn new_content_prefers_explicit_slug() {
        let mut request = NewContent::new("随便写的标题");
        request.slug = Some("Custom Slug".into());
        assert_eq!(request.source_path(), "custom-slug.md");
    }

    #[test]
    fn new_content_section_cannot_escape_content_dir() {
        let request = NewContent::new("x").in_section("../../etc");
        assert_eq!(request.source_path(), "etc/x.md");
    }

    #[test]
    fn new_content_falls_back_when_title_has_no_usable_chars() {
        assert_eq!(NewContent::new("???").source_path(), "untitled.md");
    }

    #[test]
    fn new_content_skeleton_is_parseable_and_draft_by_default() {
        let mut request = NewContent::new("带 \"引号\" 的标题").in_section("posts");
        request.tags = vec!["模板".into(), "增量".into()];
        request.description = "摘要".into();

        let raw = request.to_markdown("2026-09-07");
        let page = parse("posts/x.md", &raw);

        assert_eq!(page.title, "带 \"引号\" 的标题");
        assert_eq!(page.tags, vec!["模板", "增量"]);
        assert_eq!(page.description, "摘要");
        assert!(page.draft, "新建内容默认是草稿");
        assert!(page.date.is_some());
    }

    #[test]
    fn new_content_skeleton_omits_empty_fields() {
        let raw = NewContent::new("t").to_markdown("2026-09-07");
        assert!(!raw.contains("description"));
        assert!(!raw.contains("tags"));
        assert!(!raw.contains("template"));
    }
}
