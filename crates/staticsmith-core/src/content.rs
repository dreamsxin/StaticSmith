use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::config::SourceFormat;
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
    /// 旧地址列表。改过 slug 或搬过栏目时把老地址写在这里，构建会生成重定向页。
    #[serde(default)]
    pub aliases: Vec<String>,
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
    /// 这篇文章的旧地址。构建会为每个旧地址写一张重定向页，避免改 slug 后老链接 404。
    pub aliases: Vec<String>,
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
    pub fn from_file(
        content_root: &Path,
        source_path: &Path,
        format: SourceFormat,
    ) -> Result<Self> {
        let raw = std::fs::read_to_string(source_path).map_err(|e| Error::io(source_path, e))?;
        Self::from_str_with(content_root, source_path, &raw, format)
    }

    /// 从原始文本解析，正文按 **Markdown** 处理。
    ///
    /// 站点可能设成 HTML（`build.source_format`），那种情况下要用
    /// [`Page::from_str_with`]。这里保留一个默认 Markdown 的版本，是因为绝大多数
    /// 调用点（尤其是测试）问的就是「Markdown 解析对不对」，让它们各写一遍格式参数
    /// 只会让噪音盖住那一处真正关心格式的地方。
    pub fn from_str(content_root: &Path, source_path: &Path, raw: &str) -> Result<Self> {
        Self::from_str_with(content_root, source_path, raw, SourceFormat::Markdown)
    }

    /// 从原始文本解析，正文按 `format` 处理（便于测试与内存中的编辑器预览）。
    pub fn from_str_with(
        content_root: &Path,
        source_path: &Path,
        raw: &str,
        format: SourceFormat,
    ) -> Result<Self> {
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
        // 显式 slug 不套 slugify（会抹掉中文），但必须剔掉能逃出 href 属性的字符：
        // 模板里的 URL 位置都带 `| safe`，Tera 不会替我们转义。
        let slug = fm
            .slug
            .clone()
            .map(|s| util::sanitize_url_segment(s.trim()))
            .filter(|s| !s.is_empty())
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
        let aliases = normalize_aliases(&fm.aliases, &url);

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
            aliases,
            draft: fm.draft,
            weight: fm.weight,
            section: dir,
            is_index,
            template,
            // 站点设为 html 时原样输出：正文里写什么标签就是什么标签，`**` 就是两个星号
            content: match format {
                SourceFormat::Markdown => markdown_to_html(&body),
                SourceFormat::Html => body.clone(),
            },
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

/// 规范化旧地址。
///
/// 统一成「以 `/` 开头、以 `/` 结尾」的站内地址，与页面 URL 的形状一致；
/// 写成 `old.html` 这类带扩展名的照原样留着——那本来就是个文件。
/// 与自身 URL 相同的项被丢掉：给自己写重定向只会覆盖掉真页面。
fn normalize_aliases(raw: &[String], url: &str) -> Vec<String> {
    let mut out = Vec::new();
    for alias in raw {
        let trimmed = util::sanitize_url_segment(alias.trim());
        let trimmed = trimmed.as_str();
        if trimmed.is_empty() {
            continue;
        }
        let mut normalized = if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{trimmed}")
        };
        let last = normalized.rsplit('/').next().unwrap_or_default();
        if !normalized.ends_with('/') && !last.contains('.') {
            normalized.push('/');
        }
        if normalized == url || out.contains(&normalized) {
            continue;
        }
        out.push(normalized);
    }
    out
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

/// 递归扫描 `content/` 下的源文件并解析。
///
/// **收哪些扩展名与 `format` 无关**：`.md` / `.markdown` / `.html` / `.htm` 一律收。
/// 这样切换站点格式不必把已有文件改名，更重要的是那些「只看 front matter 与路径」的
/// 调用方（批量动作、栏目改名）即便传了默认格式，也不会漏掉 `.html` 文件——
/// 漏一篇的代价是改名后它的旧地址没人补。
///
/// `format` 只决定**正文怎么渲染**。
pub fn load_all(content_root: &Path, format: SourceFormat) -> Result<Vec<Page>> {
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
        let accepted = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("md") | Some("markdown") | Some("html") | Some("htm")
        );
        if accepted {
            pages.push(Page::from_file(content_root, path, format)?);
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

/// 清洗用户手打的源文路径：丢掉穿越片段、统一正斜杠、必要时补扩展名。
///
/// **不做 slug 化**——手打的路径就是他要的路径。补扩展名的规则只看「最后一段有没有点」，
/// 而不是维护一张合法后缀白名单：站点可以配 `source_format = "html"`，
/// 白名单迟早会漏掉某一种。
fn normalize_source_path(input: &str) -> String {
    let cleaned = util::sanitize_relative_dir(input);
    if cleaned.is_empty() {
        return "untitled.md".to_string();
    }
    let has_extension = cleaned
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'));
    if has_extension {
        cleaned
    } else {
        format!("{cleaned}.md")
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

/// 内容目录相对路径 → 绝对路径，并确保结果没有越出内容目录。
///
/// **返回 `Result` 而不是 `PathBuf` 是有意的。** `source` 可能来自 Agent、命令行参数
/// 或 IPC 调用方，两类越界都必须挡掉：
///
/// - `../../etc/passwd` 这样的相对逃逸；
/// - `C:/Windows/x` 这样的绝对路径——Windows 上 `join` 一个绝对路径会**替换整条路径**，
///   而不是拼在后面。
///
/// 曾经这里是纯拼接，越界判断放在另一个需要调用方自己想起来调的 [`is_within`] 里。
/// 结果是 8 个读写入口（桌面端与 MCP 的内容读 / 写 / 删）一个都没调它，
/// 而 core 内部的批量动作与跨文件替换反倒调了。
/// 让不安全的那个版本**不存在**，才是这类漏洞不再复发的唯一办法。
///
/// 判断是词法的（不解析符号链接）：内容目录里若有指向外部的符号链接，仍能穿出去。
/// 这一点与 `media` 模块的 `canonicalize` 双边校验不同——内容文件常常还不存在，
/// 规范化无从下手。
pub fn resolve_source(content_root: &Path, source: &str) -> Result<PathBuf> {
    let path = content_root.join(source.replace('\\', "/"));
    if !is_within(content_root, &path) {
        return Err(Error::Other(format!(
            "内容路径越出内容目录：{source}（只能写在 content/ 里面）"
        )));
    }
    Ok(path)
}

/// 给一篇内容补一条旧地址别名，返回是否真的改了文件。
///
/// 搬动文章（`batch`）与栏目改名（`sections`）都要做这件事，曾经**两处各写了一份
/// 逐字相同的实现**——别名规则改一处漏一处，代价是外部链接 404。
///
/// 规则本身在 [`crate::frontmatter::push_alias`] 里（那个模块刻意不碰磁盘），
/// 这里只负责读文件、调它、写回去。
pub fn add_alias(file: &Path, old_url: &str) -> Result<bool> {
    let raw = std::fs::read_to_string(file).map_err(|e| Error::io(file, e))?;
    match crate::frontmatter::push_alias(&raw, old_url)? {
        Some(updated) => {
            std::fs::write(file, updated).map_err(|e| Error::io(file, e))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// 这个路径是否真的落在内容目录里。
///
/// [`resolve_source`] 已经内置这项检查，这里保留为 `pub` 是给「路径不是由
/// `resolve_source` 拼出来的」那些场合用（例如遍历磁盘拿到的绝对路径）。
pub fn is_within(content_root: &Path, path: &Path) -> bool {
    match path.strip_prefix(content_root) {
        Ok(rest) => !rest
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        Err(_) => false,
    }
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
    /// **显式指定**的源文件路径（相对 `content/`），如 `posts/2026/hello.md`。
    ///
    /// 给了它就完全按它走，`section` 与 `slug` 都不再参与推导——「路径我自己写」
    /// 与「路径你替我猜」混着用，只会让人不知道最终写到哪去了。
    ///
    /// 仍然会清洗穿越片段并补 `.md`；但**不做 slug 化**：用户手打的路径就是他要的路径，
    /// 中文文件名也照原样留着（标题推导那条路才会转成 ASCII slug）。
    #[serde(default)]
    pub path: Option<String>,
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
    ///
    /// 两条互斥的路：显式 `path` 优先，否则由 `section` + (`slug` 或标题) 拼。
    pub fn source_path(&self) -> String {
        if let Some(explicit) = self
            .path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            return normalize_source_path(explicit);
        }

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

    /// 越界的 source 必须在拼路径这一步就被拒，而不是等调用方想起来查。
    ///
    /// 逐个列出来而不是只测一个 `..`：绝对路径与 Windows 盘符走的是
    /// `strip_prefix` 失败那条分支，`..` 走的是 `ParentDir` 那条，
    /// 只测一种的话另一条分支删掉了测试照样绿。
    #[test]
    fn resolve_source_rejects_paths_outside_content() {
        let root = Path::new("/site/content");
        for bad in [
            "../secret.txt",
            "../../etc/passwd",
            "posts/../../secret.txt",
            r"..\secret.txt",
            "/etc/passwd",
        ] {
            assert!(
                resolve_source(root, bad).is_err(),
                "{bad} 应当被拒绝，但通过了"
            );
        }
    }

    /// 显式路径优先，且**不做 slug 化**——手打的路径就是他要的路径。
    #[test]
    fn explicit_path_wins_over_section_and_slug() {
        let request = NewContent {
            title: "标题".into(),
            section: "posts".into(),
            slug: Some("slug".into()),
            path: Some("笔记/2026/初雪.md".into()),
            ..Default::default()
        };
        assert_eq!(request.source_path(), "笔记/2026/初雪.md");
    }

    #[test]
    fn explicit_path_gets_cleaned_and_gets_an_extension() {
        let cases = [
            ("posts/hello", "posts/hello.md"),
            ("/posts/hello.md", "posts/hello.md"),
            (r"posts\win\hello", "posts/win/hello.md"),
            ("../../etc/passwd", "etc/passwd.md"),
            ("posts/./x.md", "posts/x.md"),
            ("posts/page.html", "posts/page.html"),
            // 清洗之后什么都不剩，但**路径不是空的**（用户确实填了 `..`），
            // 这时候不该悄悄回到标题推导——那会写到一个他没要求的位置。
            ("..", "untitled.md"),
        ];
        for (input, want) in cases {
            let request = NewContent {
                title: "标题".into(),
                path: Some(input.into()),
                ..Default::default()
            };
            assert_eq!(request.source_path(), want, "输入 {input:?}");
        }
    }

    /// 纯空白的 path 等于「没填」，回到标题推导——而不是造一个 `untitled.md`。
    #[test]
    fn blank_path_falls_back_to_title() {
        for blank in ["", "   ", "\t"] {
            let request = NewContent {
                title: "标题".into(),
                path: Some(blank.into()),
                ..Default::default()
            };
            assert_eq!(request.source_path(), "标题.md", "输入 {blank:?}");
        }
    }

    /// 没给 path 时走原来那条推导。
    ///
    /// 注意**文件名保留中文**：`util::slugify_name` 只清标点，不转写。
    /// 而 URL 那一侧走的是 `slug::slugify`，中文会被转成拼音——两个 slug 函数、
    /// 两种结果，所以「文件名」与「网址」对中文标题天然不一致。
    /// 这条测试把现状钉住，改动 URL 生成会破坏已有站点的地址，不能顺手改。
    #[test]
    fn without_path_it_still_derives_from_title() {
        let request = NewContent::new("你好，世界").in_section("posts");
        assert_eq!(request.source_path(), "posts/你好-世界.md");
    }

    #[test]
    fn resolve_source_keeps_normal_relative_paths() {
        let root = Path::new("/site/content");
        assert_eq!(
            resolve_source(root, "posts/hello.md").unwrap(),
            root.join("posts/hello.md")
        );
        // 反斜杠是 Windows 上界面传过来的常态，统一成正斜杠后仍在目录内。
        assert_eq!(
            resolve_source(root, r"posts\hello.md").unwrap(),
            root.join("posts/hello.md")
        );
    }

    /// 显式 slug 里的引号会逃出 `href` 属性——模板那些位置带 `| safe`，没人替它转义。
    #[test]
    fn explicit_slug_cannot_break_out_of_an_attribute() {
        let raw = "+++\ntitle = \"注入\"\nslug = 'a\" onmouseover=\"alert(1)'\n+++\n正文\n";
        let page =
            Page::from_str(Path::new("content"), Path::new("content/posts/x.md"), raw).unwrap();

        assert!(!page.url.contains('"'), "URL 里不该留下引号：{}", page.url);
        assert!(
            !page.output.contains('"'),
            "产物路径里不该留下引号：{}",
            page.output
        );
        assert_eq!(page.url, "/posts/a onmouseover=alert(1)/");
    }

    /// 中文 slug 必须原样保留：套 slugify 会把它抹成空串，整站 URL 都变。
    #[test]
    fn non_ascii_slug_survives() {
        let raw = "+++\ntitle = \"中文\"\nslug = \"你好\"\n+++\n正文\n";
        let page =
            Page::from_str(Path::new("content"), Path::new("content/posts/x.md"), raw).unwrap();

        assert_eq!(page.url, "/posts/你好/");
    }

    /// 站点设为 html 时正文原样输出，与 Markdown 那条路真的不同。
    ///
    /// 两条路各断言一次，而不是只断言 HTML 那条：只测一边的话，
    /// 哪天 `markdown_to_html` 自己变成「原样返回」，测试照样绿。
    #[test]
    fn html_format_emits_body_verbatim() {
        let raw = "+++\ntitle = \"原样\"\n+++\n**加粗**\n";

        let html = Page::from_str_with(
            Path::new("content"),
            Path::new("content/raw.md"),
            raw,
            SourceFormat::Html,
        )
        .unwrap();
        assert!(html.content.contains("**加粗**"), "`**` 应当保持原样");
        assert!(!html.content.contains("<strong>"), "不该跑 Markdown");
        assert!(!html.content.contains("<p>"), "也不该自动包段落");

        let markdown =
            Page::from_str(Path::new("content"), Path::new("content/raw.md"), raw).unwrap();
        assert!(markdown.content.contains("<strong>"), "Markdown 那条路照旧");
    }

    fn parse(path: &str, raw: &str) -> Page {
        Page::from_str(
            Path::new("/site/content"),
            &Path::new("/site/content").join(path),
            raw,
        )
        .unwrap()
    }

    #[test]
    fn aliases_are_normalized_to_site_urls() {
        let page = parse(
            "posts/new-slug.md",
            "+++\ntitle = \"改过地址\"\naliases = [\"old-slug\", \"/posts/older/\", \"legacy.html\", \"\", \"/posts/new-slug/\"]\n+++\n正文\n",
        );
        assert_eq!(
            page.aliases,
            vec![
                "/old-slug/".to_string(),
                "/posts/older/".to_string(),
                "/legacy.html".to_string(),
            ],
            "补斜杠、保留带扩展名的、丢掉空串与指向自己的"
        );
    }

    #[test]
    fn aliases_do_not_become_a_taxonomy_dimension() {
        // aliases 是字符串数组，若不声明成字段会被当成自定义分类维度
        let page = parse(
            "posts/a.md",
            "+++\ntitle = \"甲\"\naliases = [\"/old/\"]\n+++\n正文\n",
        );
        assert!(
            !page.taxonomies.contains_key("aliases"),
            "{:?}",
            page.taxonomies
        );
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
