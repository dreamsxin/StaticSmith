//! 内存预览：把本地资源就地嵌进 HTML。
//!
//! 内存预览走 iframe 的 `srcdoc`，那是个没有文件访问权限的沙箱：`<link>` 的 CSS
//! 取不到、`<img>` 的图片取不到。于是「预览」只能看结构，看不出实际长相——
//! 写字的人以为图片丢了，改主题的人以为样式没生效（`docs/ui-review.md` 第 2 条）。
//!
//! 这里把 HTML 里指向本地文件的引用替换成内联内容：CSS 变 `<style>`，
//! 图片与字体变 data URL。三条自我约束：
//!
//! 1. **只碰本地引用**。`http(s)://`、`//`、`data:`、`#`、`mailto:` 一律不动——
//!    内存预览不该悄悄替用户发网络请求。
//! 2. **有预算**。单个文件超过 [`MAX_FILE_BYTES`]、累计超过 [`MAX_TOTAL_BYTES`] 就
//!    放弃内联（原样留着那个地址）。base64 会把体积撑到 4/3，一个 20 MB 的视频
//!    嵌进 `srcdoc` 会让整个界面卡住。
//! 3. **取不到就原样留着**，不报错、不塞占位图。预览是辅助视图，不该因为少一张图
//!    就打断写作；而留着原地址，浏览器控制台里仍能看出「这里本来要加载什么」。
//!
//! 已知不做的：`srcset`（多候选地址，挑哪一个取决于 DPR，猜错比不做更糟）、
//! CSS 里的 `@import`（嵌套层级不定，收益也低）。两者都留在文档里。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::config::ProjectPaths;
use crate::util;

/// 单个文件的上限。超过就不内联——预览里一张 2 MB 的图已经看得清了。
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 一次预览的累计上限。`srcdoc` 是一个字符串，太大之后 WebView 解析会明显卡顿。
pub const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;

/// 内联结果：换掉了几处、放弃了几处。
///
/// 界面要能说清「这一屏是不是完整效果」，所以放弃的处数必须回传，
/// 而不是让用户对着一张缺图的预览猜。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Inlined {
    pub replaced: usize,
    pub skipped: usize,
}

/// 把 HTML 里的本地引用换成内联内容。
pub fn inline_assets(html: &str, paths: &ProjectPaths) -> (String, Inlined) {
    let mut ctx = Ctx {
        roots: roots(paths),
        cache: HashMap::new(),
        used: 0,
        stat: Inlined::default(),
    };
    let out = rewrite_tags(html, &mut ctx);
    (out, ctx.stat)
}

/// URL 根目录的查找顺序，与构建时 `copy_assets` 的覆盖顺序一致：
/// 站点自己的 `static/` 压在主题的 `static/` 之上。顺序不一致就会出现
/// 「预览用的是主题那份、产物用的是我改过的那份」。
fn roots(paths: &ProjectPaths) -> Vec<PathBuf> {
    vec![paths.static_dir.clone(), paths.theme.join("static")]
}

struct Ctx {
    roots: Vec<PathBuf>,
    /// 同一份文件在一页里可能被引用多次（列表页的占位图），编码一次就够
    cache: HashMap<PathBuf, Option<String>>,
    used: u64,
    stat: Inlined,
}

/// 逐个标签扫过去，把要改的整段替换掉。
///
/// 自己扫而不引 HTML 解析器：这里只需要认出 `<link>` 与带 `src` 的标签，
/// 引一个 DOM 库要为「找两个属性」付上百 KB 依赖与一次完整的树往返
/// （往返还会顺手改写用户模板里的写法）。扫描时跟踪引号状态，
/// 所以属性值里的 `>` 不会把标签切断。
fn rewrite_tags(html: &str, ctx: &mut Ctx) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut at = 0usize;

    while at < bytes.len() {
        let Some(open) = html[at..].find('<').map(|i| at + i) else {
            out.push_str(&html[at..]);
            break;
        };
        out.push_str(&html[at..open]);
        let Some(close) = tag_end(html, open) else {
            // 没有闭合的 `<`：原样收尾，不猜
            out.push_str(&html[open..]);
            break;
        };
        let tag = &html[open..=close];
        match rewrite_tag(tag, ctx) {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(tag),
        }
        at = close + 1;
    }
    out
}

/// 标签的结束 `>` 位置。引号里的 `>` 不算。
fn tag_end(html: &str, open: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (offset, ch) in html[open..].char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(ch),
            (None, '>') => return Some(open + offset),
            (None, _) => {}
        }
    }
    None
}

/// 单个标签的替换结果。`None` 表示不动它。
fn rewrite_tag(tag: &str, ctx: &mut Ctx) -> Option<String> {
    let name = tag_name(tag)?;
    if name == "link" {
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        if !rel.split_whitespace().any(|word| word == "stylesheet") {
            return None;
        }
        let href = attr(tag, "href")?;
        let path = resolve(href, &ctx.roots)?;
        let css = read_text(&path, ctx)?;
        // 样式表里的 url() 也要跟着内联，否则背景图与字体照旧是空的
        let css = inline_css_urls(&css, &path, ctx);
        ctx.stat.replaced += 1;
        return Some(format!("<style>{css}</style>"));
    }

    let src = attr(tag, "src")?;
    let path = resolve(src, &ctx.roots)?;
    let data = read_data_url(&path, ctx)?;
    ctx.stat.replaced += 1;
    Some(replace_attr(tag, "src", &data))
}

fn tag_name(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix('<')?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

/// 取属性值。只认带引号的写法——模板渲染出来的属性一律带引号，
/// 为无引号写法多写一套解析等于为不会出现的输入付复杂度。
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let (start, end) = attr_span(tag, name)?;
    Some(&tag[start..end])
}

fn attr_span(tag: &str, name: &str) -> Option<(usize, usize)> {
    let lower = tag.to_ascii_lowercase();
    // 属性名前必须是空白，否则 `data-src` 会命中 `src`
    let mut from = 0usize;
    loop {
        let found = lower[from..].find(name).map(|i| from + i)?;
        let before = lower[..found].chars().last();
        let after = lower[found + name.len()..].trim_start();
        let boundary = before.is_some_and(char::is_whitespace);
        if boundary && after.starts_with('=') {
            let eq = found + name.len() + lower[found + name.len()..].find('=')?;
            let rest = &tag[eq + 1..];
            let quote = rest.trim_start().chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let value_start = eq + 1 + rest.find(quote)? + 1;
            let value_end = value_start + tag[value_start..].find(quote)?;
            return Some((value_start, value_end));
        }
        from = found + name.len();
    }
}

fn replace_attr(tag: &str, name: &str, value: &str) -> String {
    match attr_span(tag, name) {
        Some((start, end)) => format!("{}{}{}", &tag[..start], value, &tag[end..]),
        None => tag.to_string(),
    }
}

/// 把地址解析成磁盘路径。非本地地址、越界路径、不存在的文件都返回 `None`。
fn resolve(url: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    // 外部地址与内联数据一律不动：内存预览不该替用户发网络请求
    if lower.starts_with("data:")
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("//")
        || lower.starts_with("mailto:")
        || lower.starts_with("javascript:")
        || url.starts_with('#')
    {
        return None;
    }
    // 查询串与锚点不参与寻址（`?v=2` 这类缓存参数很常见）
    let path = url.split(['?', '#']).next()?.trim_start_matches('/');
    if path.is_empty() || path.split('/').any(|seg| seg == "..") {
        return None;
    }
    roots
        .iter()
        .map(|root| root.join(path))
        .find(|candidate| candidate.is_file())
}

/// 读文本文件，同时记账。超预算返回 `None`。
fn read_text(path: &Path, ctx: &mut Ctx) -> Option<String> {
    if !afford(path, ctx) {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            ctx.used += text.len() as u64;
            Some(text)
        }
        Err(_) => {
            ctx.stat.skipped += 1;
            None
        }
    }
}

/// 读二进制并编码成 data URL。同一份文件只编码一次。
fn read_data_url(path: &Path, ctx: &mut Ctx) -> Option<String> {
    if let Some(cached) = ctx.cache.get(path) {
        return cached.clone();
    }
    let made = encode_data_url(path, ctx);
    ctx.cache.insert(path.to_path_buf(), made.clone());
    made
}

fn encode_data_url(path: &Path, ctx: &mut Ctx) -> Option<String> {
    if !afford(path, ctx) {
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            ctx.stat.skipped += 1;
            return None;
        }
    };
    ctx.used += bytes.len() as u64;
    let mime = util::mime_for(path);
    Some(format!("data:{mime};base64,{}", BASE64.encode(&bytes)))
}

/// 预算检查。放弃时记一笔 `skipped`——界面要能说清「这一屏不是完整效果」。
fn afford(path: &Path, ctx: &mut Ctx) -> bool {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    if size > MAX_FILE_BYTES || ctx.used + size > MAX_TOTAL_BYTES {
        ctx.stat.skipped += 1;
        return false;
    }
    true
}

/// 把 CSS 里的 `url(...)` 换成 data URL。相对地址按**这份 CSS 所在目录**解析。
fn inline_css_urls(css: &str, css_path: &Path, ctx: &mut Ctx) -> String {
    let base = css_path.parent().map(Path::to_path_buf);
    let mut out = String::with_capacity(css.len());
    let mut at = 0usize;
    while let Some(found) = css[at..].find("url(").map(|i| at + i) {
        let open = found + 4;
        let Some(close) = css[open..].find(')').map(|i| open + i) else {
            break;
        };
        let raw = css[open..close].trim();
        let quote = raw.starts_with('"') || raw.starts_with('\'');
        let url = raw.trim_matches(['"', '\'']);
        let roots = match &base {
            // CSS 自己所在的目录排在最前：`url(../fonts/x.woff2)` 这类写法要按它解析
            Some(dir) => {
                let mut list = vec![dir.clone()];
                list.extend(ctx.roots.iter().cloned());
                list
            }
            None => ctx.roots.clone(),
        };
        out.push_str(&css[at..found]);
        match resolve_css_url(url, &roots).and_then(|path| read_data_url(&path, ctx)) {
            Some(data) => {
                out.push_str("url(");
                if quote {
                    out.push('"');
                    out.push_str(&data);
                    out.push('"');
                } else {
                    out.push_str(&data);
                }
                out.push(')');
                ctx.stat.replaced += 1;
            }
            None => out.push_str(&css[found..=close]),
        }
        at = close + 1;
    }
    out.push_str(&css[at..]);
    out
}

/// CSS 里的地址：允许 `../`（相对 CSS 自己的位置），但仍不许跳出候选根目录。
fn resolve_css_url(url: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if url.is_empty()
        || lower.starts_with("data:")
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("//")
        || url.starts_with('#')
    {
        return None;
    }
    let path = url.split(['?', '#']).next()?;
    let absolute = path.starts_with('/');
    let trimmed = path.trim_start_matches('/');
    for root in roots {
        let candidate = root.join(trimmed);
        // 规范化之后必须仍在某个候选根目录里：CSS 里写 `../../../etc/passwd`
        // 不该让预览把系统文件嵌进来
        let Ok(real) = candidate.canonicalize() else {
            continue;
        };
        if !real.is_file() {
            continue;
        }
        if roots
            .iter()
            .filter_map(|r| r.canonicalize().ok())
            .any(|r| real.starts_with(&r))
        {
            return Some(real);
        }
        if absolute {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Assets, Build};

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: ProjectPaths,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), &Build::default(), &Assets::default());
        std::fs::create_dir_all(paths.static_dir.join("css")).unwrap();
        std::fs::create_dir_all(paths.static_dir.join("img")).unwrap();
        Fixture { _dir: dir, paths }
    }

    /// 1×1 的 PNG，够小、够真。
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52,
    ];

    #[test]
    fn stylesheet_becomes_style_and_image_becomes_data_url() {
        let f = fixture();
        std::fs::write(f.paths.static_dir.join("css/site.css"), "body{color:red}").unwrap();
        std::fs::write(f.paths.static_dir.join("img/a.png"), PNG).unwrap();

        let html = r#"<link rel="stylesheet" href="/css/site.css"><img src="/img/a.png" alt="图">"#;
        let (out, stat) = inline_assets(html, &f.paths);

        assert!(out.contains("<style>body{color:red}</style>"), "{out}");
        assert!(out.contains("src=\"data:image/png;base64,"), "{out}");
        assert!(out.contains("alt=\"图\""), "其余属性要原样保留：{out}");
        assert_eq!(stat.replaced, 2);
        assert_eq!(stat.skipped, 0);
    }

    #[test]
    fn external_and_data_urls_are_left_alone() {
        let f = fixture();
        let html = concat!(
            r#"<img src="https://example.com/a.png">"#,
            r#"<img src="data:image/gif;base64,AA">"#,
            r#"<link rel="preconnect" href="https://cdn.example.com">"#,
        );
        let (out, stat) = inline_assets(html, &f.paths);
        assert_eq!(out, html, "外部地址不该被动");
        assert_eq!(stat.replaced, 0);
    }

    #[test]
    fn site_static_wins_over_theme() {
        let f = fixture();
        let theme_css = f.paths.theme.join("static/css");
        std::fs::create_dir_all(&theme_css).unwrap();
        std::fs::write(theme_css.join("site.css"), "body{color:主题}").unwrap();
        std::fs::write(f.paths.static_dir.join("css/site.css"), "body{color:站点}").unwrap();

        let (out, _) = inline_assets(r#"<link rel="stylesheet" href="/css/site.css">"#, &f.paths);
        // 与构建时 copy_assets 的覆盖顺序一致：站点压主题
        assert!(out.contains("站点"), "{out}");
        assert!(!out.contains("主题"), "{out}");
    }

    #[test]
    fn css_url_references_are_inlined_relative_to_the_stylesheet() {
        let f = fixture();
        std::fs::write(
            f.paths.static_dir.join("css/site.css"),
            "body{background:url(../img/a.png)}",
        )
        .unwrap();
        std::fs::write(f.paths.static_dir.join("img/a.png"), PNG).unwrap();

        let (out, stat) =
            inline_assets(r#"<link rel="stylesheet" href="/css/site.css">"#, &f.paths);
        assert!(out.contains("url(data:image/png;base64,"), "{out}");
        assert_eq!(stat.replaced, 2, "样式表本身 + 它引用的图");
    }

    #[test]
    fn oversized_files_are_skipped_and_counted() {
        let f = fixture();
        let big = vec![0u8; (MAX_FILE_BYTES + 1) as usize];
        std::fs::write(f.paths.static_dir.join("img/big.png"), &big).unwrap();

        let html = r#"<img src="/img/big.png">"#;
        let (out, stat) = inline_assets(html, &f.paths);
        assert_eq!(out, html, "放弃内联时要原样留着地址");
        assert_eq!(stat.skipped, 1);
        assert_eq!(stat.replaced, 0);
    }

    #[test]
    fn traversal_is_refused() {
        let f = fixture();
        std::fs::write(f.paths.root.join("secret.txt"), "机密").unwrap();
        let html = r#"<img src="/../secret.txt">"#;
        let (out, _) = inline_assets(html, &f.paths);
        assert_eq!(out, html);
    }

    #[test]
    fn a_greater_than_inside_an_attribute_does_not_split_the_tag() {
        let f = fixture();
        std::fs::write(f.paths.static_dir.join("img/a.png"), PNG).unwrap();
        let html = r#"<img src="/img/a.png" title="1 > 0"><p>后面</p>"#;
        let (out, stat) = inline_assets(html, &f.paths);
        assert_eq!(stat.replaced, 1, "{out}");
        assert!(out.contains(r#"title="1 > 0""#), "{out}");
        assert!(out.ends_with("<p>后面</p>"), "{out}");
    }

    #[test]
    fn data_src_is_not_mistaken_for_src() {
        let f = fixture();
        std::fs::write(f.paths.static_dir.join("img/a.png"), PNG).unwrap();
        let html = r#"<img data-src="/img/a.png">"#;
        let (out, stat) = inline_assets(html, &f.paths);
        assert_eq!(out, html, "属性名要整词匹配：{out}");
        assert_eq!(stat.replaced, 0);
    }
}
