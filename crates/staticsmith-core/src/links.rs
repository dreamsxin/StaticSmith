//! 站内链接体检：找出点了会 404 的内部链接。
//!
//! SEO 体检看的是「这篇文章的元数据写全了没」，媒体体检看的是「图还在不在」，
//! 但站点运营里最常见的坏味道是第三种：改了 slug、删了旧文、写错相对路径，
//! 于是导航里留下一个点了就 404 的链接。搜索引擎会因此降权，读者会直接流失。
//!
//! 判定依据是产物目录，而不是内容源文件——只有产物才知道分页页、标签页、
//! `slug` 覆盖之后的真实地址。因此必须先生成一次；没生成过时报告里
//! `built = false`，界面据此提示「先生成」而不是谎报「零死链」。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use walkdir::WalkDir;

use crate::error::Result;
use crate::outputs;
use crate::util;

/// 一条断链，以及所有引用了它的页面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokenLink {
    /// 页面里原样写着的地址（保留 `../`、查询串等，方便搜到出处）
    pub href: String,
    /// 解析后的站内地址，即实际找不到的目标
    pub url: String,
    /// 引用它的页面地址，按字典序
    pub referenced_by: Vec<String>,
}

/// 体检报告。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// 产物目录是否存在。为 `false` 时其余字段都是 0，代表「还没生成过」
    pub built: bool,
    /// 扫过的 HTML 页面数
    pub pages: usize,
    /// 站内链接总数（去重前）
    pub internal: usize,
    /// 站外链接总数，仅计数，不发网络请求
    pub external: usize,
    pub broken: Vec<BrokenLink>,
}

impl Report {
    /// 没生成过时的空报告。
    fn not_built() -> Self {
        Self {
            built: false,
            pages: 0,
            internal: 0,
            external: 0,
            broken: Vec::new(),
        }
    }
}

/// 体检。只读产物目录，不发网络请求，也不写盘。
///
/// `base_url` 用来把模板里写死的绝对地址（canonical、og:url）算作站内链接；
/// 留空则这类地址计入 `external`。
pub fn audit(output_dir: &Path, base_url: &str) -> Result<Report> {
    if !output_dir.is_dir() {
        return Ok(Report::not_built());
    }

    let existing = collect_files(output_dir);
    let site = base_url.trim().trim_end_matches('/').to_string();

    let mut pages = 0;
    let mut internal = 0;
    let mut external = 0;
    let mut broken: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();

    for path in existing.iter().filter(|p| p.ends_with(".html")) {
        let absolute = path
            .split('/')
            .fold(output_dir.to_path_buf(), |acc, s| acc.join(s));
        let Ok(html) = std::fs::read_to_string(&absolute) else {
            continue;
        };
        pages += 1;
        let from = outputs::url_for(path);

        for href in extract_links(&html) {
            match classify(&href, &site) {
                Target::External => external += 1,
                Target::Ignored => {}
                Target::Internal(target) => {
                    internal += 1;
                    let Some(url) = resolve(&from, &target) else {
                        continue;
                    };
                    if !exists(&url, &existing) {
                        broken
                            .entry((href.clone(), url))
                            .or_default()
                            .insert(from.clone());
                    }
                }
            }
        }
    }

    Ok(Report {
        built: true,
        pages,
        internal,
        external,
        broken: broken
            .into_iter()
            .map(|((href, url), by)| BrokenLink {
                href,
                url,
                referenced_by: by.into_iter().collect(),
            })
            .collect(),
    })
}

/// 链接的去向。
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// 站内地址，已去掉 `base_url` 前缀与锚点/查询串
    Internal(String),
    External,
    /// 纯锚点、`mailto:` 等根本不指向页面的地址
    Ignored,
}

/// 判断链接去向。
fn classify(href: &str, site: &str) -> Target {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') {
        return Target::Ignored;
    }
    // 协议相对地址 //cdn.example.com/x 一律算站外
    if href.starts_with("//") {
        return Target::External;
    }
    if let Some(scheme_end) = href.find(':') {
        let scheme = &href[..scheme_end];
        let is_scheme = !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        // Windows 盘符不会出现在 HTML 里，但 `foo:bar` 这类相对路径要排除误判
        if is_scheme && href[scheme_end..].starts_with("://") {
            if !site.is_empty() {
                if let Some(rest) = href.strip_prefix(site) {
                    if rest.is_empty() || rest.starts_with('/') || rest.starts_with('#') {
                        let rest = if rest.is_empty() { "/" } else { rest };
                        return Target::Internal(strip_tail(rest));
                    }
                }
            }
            return Target::External;
        }
        if is_scheme {
            // mailto:、tel:、javascript:、data: 之类
            return Target::Ignored;
        }
    }
    Target::Internal(strip_tail(href))
}

/// 去掉锚点与查询串。静态站点没有服务端，`?a=1` 不改变目标文件。
fn strip_tail(url: &str) -> String {
    let cut = url.find(['#', '?']).unwrap_or(url.len());
    url[..cut].to_string()
}

/// 把站内地址解析成相对输出根的地址（以 `/` 开头）。
///
/// 越过站点根的 `../` 返回 `None`——那不是死链而是写法错误，
/// 单独报会更吵，交给「找不到」这一条结论就够了。
fn resolve(from: &str, target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    if !target.starts_with('/') {
        // from 形如 /posts/a/ 或 /sitemap.xml，取其所在目录
        let dir = from.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        segments.extend(dir.split('/').filter(|s| !s.is_empty()));
    }
    let trailing = target.ends_with('/');
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }

    let mut url = format!("/{}", segments.join("/"));
    if trailing && !url.ends_with('/') {
        url.push('/');
    }
    Some(url)
}

/// 站内地址在产物里有没有对应文件。
fn exists(url: &str, existing: &BTreeSet<String>) -> bool {
    let path = url.trim_start_matches('/');
    if path.is_empty() || url.ends_with('/') {
        return existing.contains(&format!("{path}index.html"));
    }
    existing.contains(path)
        // /about 也能打开 /about/index.html：多数静态托管会补重定向
        || existing.contains(&format!("{path}/index.html"))
}

/// 产物目录里的全部文件，相对根的正斜杠路径。
fn collect_files(output_dir: &Path) -> BTreeSet<String> {
    WalkDir::new(output_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| util::to_slash(e.path().strip_prefix(output_dir).unwrap_or(e.path())))
        .collect()
}

/// 抓出 `href="…"` / `src="…"` 的值。
///
/// 这里刻意不引入 HTML 解析器：产物是自家模板渲染的，属性一定带引号，
/// 而一个几十行的扫描器不会在依赖树里塞进一整套解析器。
///
/// 全程按字节走且只在 ASCII 位置切片——正文里全是中文时，按字符数猜偏移
/// 会直接切在 UTF-8 字符中间。
fn extract_links(html: &str) -> Vec<String> {
    const ATTRS: [&[u8]; 2] = [b"href", b"src"];
    let bytes = html.as_bytes();
    let mut links = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        // 属性名前必须是空白，否则 data-src、nohref 也会被算进来
        if !bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let Some(attr) = ATTRS
            .into_iter()
            .find(|a| bytes.len() > j + a.len() && bytes[j..j + a.len()].eq_ignore_ascii_case(a))
        else {
            i += 1;
            continue;
        };
        j += attr.len();

        j = skip_spaces(bytes, j);
        if bytes.get(j) != Some(&b'=') {
            i += 1;
            continue;
        }
        j = skip_spaces(bytes, j + 1);

        let Some(&quote) = bytes.get(j).filter(|c| matches!(c, b'"' | b'\'')) else {
            // 未加引号的属性值：产物里不会出现，跳过即可
            i = j.max(i + 1);
            continue;
        };
        j += 1;
        let start = j;
        while j < bytes.len() && bytes[j] != quote {
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        // 引号是 ASCII，所以 start..j 一定落在字符边界上
        links.push(unescape(&html[start..j]));
        i = j + 1;
    }
    links
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

/// 把 HTML 实体还原成字符。模板 autoescape 会把 `&` 写成 `&amp;`。
fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, text: &str) {
        let target = path
            .split('/')
            .fold(root.to_path_buf(), |acc, s| acc.join(s));
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }

    #[test]
    fn missing_output_dir_reports_not_built() {
        let dir = tempfile::tempdir().unwrap();
        let report = audit(&dir.path().join("dist"), "").unwrap();
        assert!(!report.built);
        assert_eq!(report.pages, 0);
        assert!(report.broken.is_empty());
    }

    #[test]
    fn finds_broken_links_and_lists_every_referrer() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "index.html",
            r#"<a href="/posts/a/">a</a><a href="/posts/gone/">没了</a>"#,
        );
        write(
            root,
            "posts/a/index.html",
            r#"<a href="/posts/gone/">也没了</a><a href="/">回首页</a>"#,
        );

        let report = audit(root, "").unwrap();
        assert!(report.built);
        assert_eq!(report.pages, 2);
        assert_eq!(report.internal, 4);
        assert_eq!(report.broken.len(), 1);
        assert_eq!(report.broken[0].url, "/posts/gone/");
        assert_eq!(report.broken[0].referenced_by, vec!["/", "/posts/a/"]);
    }

    #[test]
    fn relative_links_resolve_against_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "about/index.html", "关于");
        write(
            root,
            "posts/a/index.html",
            r#"<a href="../../about/">关于</a><a href="../b/">兄弟</a>"#,
        );

        let report = audit(root, "").unwrap();
        assert_eq!(report.broken.len(), 1);
        assert_eq!(report.broken[0].url, "/posts/b/");
        assert_eq!(report.broken[0].href, "../b/");
    }

    #[test]
    fn external_links_are_counted_not_checked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "index.html",
            r##"<a href="https://example.com/x">外链</a><a href="//cdn.example.com/a.js">协议相对</a>
               <a href="mailto:a@b.c">邮件</a><a href="#top">锚点</a>"##,
        );

        let report = audit(root, "").unwrap();
        assert_eq!(report.external, 2);
        assert_eq!(report.internal, 0);
        assert!(report.broken.is_empty());
    }

    #[test]
    fn absolute_links_to_our_own_base_url_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "index.html",
            r#"<link href="https://example.com/" /><a href="https://example.com/posts/gone/">x</a>"#,
        );

        let report = audit(root, "https://example.com/").unwrap();
        assert_eq!(report.external, 0);
        assert_eq!(report.internal, 2);
        assert_eq!(report.broken.len(), 1);
        assert_eq!(report.broken[0].url, "/posts/gone/");

        // 没配 base_url 时同样的地址算站外，不会误报
        let report = audit(root, "").unwrap();
        assert_eq!(report.external, 2);
        assert!(report.broken.is_empty());
    }

    #[test]
    fn query_and_fragment_do_not_affect_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "index.html",
            r#"<a href="/about/?utm=1#top">关于</a>"#,
        );
        write(root, "about/index.html", "关于");

        assert!(audit(root, "").unwrap().broken.is_empty());
    }

    #[test]
    fn assets_and_extensionless_urls_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "css/main.css", "body{}");
        write(root, "about/index.html", "关于");
        write(
            root,
            "index.html",
            r#"<link href="/css/main.css"><script src="/js/app.js"></script><a href="/about">关于</a>"#,
        );

        let report = audit(root, "").unwrap();
        assert_eq!(report.broken.len(), 1, "{:?}", report.broken);
        assert_eq!(report.broken[0].url, "/js/app.js");
    }

    #[test]
    fn escaped_ampersands_are_restored() {
        let links = extract_links(r#"<a href="/a/?x=1&amp;y=2">x</a>"#);
        assert_eq!(links, vec!["/a/?x=1&y=2".to_string()]);
    }

    #[test]
    fn data_src_is_not_mistaken_for_src() {
        let links = extract_links(r#"<img data-src="/lazy.png" src="/real.png">"#);
        assert_eq!(links, vec!["/real.png".to_string()]);
    }

    #[test]
    fn chinese_text_does_not_break_offsets() {
        // 中文是 3 字节，按字符数猜偏移会切在字符中间——这里守住这条
        let links = extract_links(
            "<p>正文里有很长一段中文，用来把字节偏移和字符偏移拉开距离。</p>\
             <a href=\"/关于/\">关于我们</a><p>后面还有中文。</p><a href='/联系/'>联系</a>",
        );
        assert_eq!(links, vec!["/关于/".to_string(), "/联系/".to_string()]);
    }

    #[test]
    fn traversal_above_root_is_not_reported_twice() {
        assert_eq!(resolve("/posts/a/", "../../.."), None);
        assert_eq!(resolve("/posts/a/", "../"), Some("/posts/".to_string()));
        assert_eq!(resolve("/", "about/"), Some("/about/".to_string()));
    }
}
