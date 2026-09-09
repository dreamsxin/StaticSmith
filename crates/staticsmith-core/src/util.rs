use std::path::Path;

use sha2::{Digest, Sha256};

/// 统一使用正斜杠表示相对路径：Windows 与 macOS 的索引键、模板名必须一致。
pub fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// 静态站点会用到的 MIME 类型。未知扩展名按二进制流处理。
///
/// 放在 util 而不是 serve：预览服务器要它填响应头，内存预览要它拼 data URL
/// （[`crate::preview`]），而 `serve` 是可选特性——两处各写一份扩展名表，
/// 迟早出现「服务器认得 .avif、内存预览不认」。
pub fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// 内容哈希（截取前 16 字节十六进制，足够做变更判定且节省索引空间）。
pub fn hash_str(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    hex16(&digest)
}

pub fn hash_bytes(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    hex16(&digest)
}

/// 完整的 SHA-256 十六进制串（64 字符）。///
/// 资源去重用全长哈希：截断到 16 字节虽然足够做变更判定，但资源是内容寻址的存储键，
/// 不值得为省几十字节承担额外碰撞面。
pub fn hash_bytes_full(input: &[u8]) -> String {
    Sha256::digest(input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hex16(digest: &[u8]) -> String {
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// 生成用于文件名与 URL 的名字：保留字母数字（含中日韩）与 `-` `_`，其余折叠为单个 `-`。
///
/// 不做音译（`slug` crate 会把中文整段丢掉），因此中文标题会原样保留在 URL 里——
/// 现代浏览器与服务器都能正确处理 UTF-8 路径，硬转拼音反而丢信息。
pub fn slugify_name(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.chars().count() >= 64 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// 清洗要拼进 URL 与 HTML 属性的那一段（slug、别名）。
///
/// 起因是一个真的能利用的注入点：`slug::slugify` 只作用于**兜底的文件名**，
/// front matter 里显式写的 `slug` 一路直通到模板，而模板里那些位置都带 `| safe`
/// （URL 被 Tera 转义成 `&#x2F;` 就不能用了）。于是
/// `slug = 'a" onmouseover="alert(1)'` 会渲染成 `href="/posts/a" onmouseover="..."`，
/// 逃出了属性。
///
/// 这里**不套 `slugify`**：那会把中文 slug 整段抹成空串，用中文文件名的站点 URL 全变。
/// 只剔掉能破坏属性或路径的那几个字符，非 ASCII 原样保留。
pub fn sanitize_url_segment(input: &str) -> String {
    input
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '<' | '>' | '`' | '\\') && !c.is_control())
        .collect()
}

/// 清洗相对目录：统一正斜杠、去掉首尾斜杠与 `.` / `..` 片段。
///
/// 用于所有来自界面输入的目录（新建内容的栏目、资源子目录），杜绝路径穿越。
pub fn sanitize_relative_dir(input: &str) -> String {
    input
        .replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .collect::<Vec<_>>()
        .join("/")
}

/// 保守的 HTML 压缩：折叠标签间的空白，保留 `pre` / `code` / `script` / `style` / `textarea` 内部原样。
///
/// 不做属性重写或标签省略，避免破坏用户在主题里手写的 JS/CSS。
pub fn minify_html(html: &str) -> String {
    const KEEP: [&str; 5] = ["pre", "code", "script", "style", "textarea"];
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut keep_depth = 0usize;
    let mut pending_space = false;

    for (i, ch) in html.char_indices() {
        if ch == '<' {
            let rest = &lower[i..];
            if KEEP.iter().any(|t| opens_tag(rest, t)) {
                keep_depth += 1;
            } else if KEEP.iter().any(|t| rest.starts_with(&format!("</{t}"))) {
                keep_depth = keep_depth.saturating_sub(1);
            }
            pending_space = false;
            out.push('<');
            continue;
        }

        if keep_depth == 0 && matches!(ch, ' ' | '\t' | '\r' | '\n') {
            pending_space = true;
            continue;
        }
        if pending_space {
            // 文本节点内部保留单个空格，避免 `hello world` 被粘成 `helloworld`。
            if !out.ends_with('>') {
                out.push(' ');
            }
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

/// 判断 `<tag` 开标签，排除 `<code-block` 这类同前缀自定义元素。
fn opens_tag(rest_lower: &str, tag: &str) -> bool {
    rest_lower
        .strip_prefix('<')
        .and_then(|s| s.strip_prefix(tag))
        .is_some_and(|after| after.starts_with('>') || after.starts_with(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_paths_are_platform_independent() {
        assert_eq!(
            to_slash(Path::new("posts").join("a.md").as_path()),
            "posts/a.md"
        );
    }

    #[test]
    fn hash_is_stable_and_short() {
        let h = hash_str("abc");
        assert_eq!(h.len(), 32);
        assert_eq!(h, hash_str("abc"));
    }

    #[test]
    fn minify_collapses_whitespace_between_tags() {
        let out = minify_html("<div>\n  <p>你好</p>\n</div>\n");
        assert_eq!(out, "<div><p>你好</p></div>");
    }

    #[test]
    fn minify_preserves_pre_and_script_bodies() {
        let src = "<pre>line1\n  line2</pre><script>\nvar a = 1;\n</script>";
        assert_eq!(minify_html(src), src);
    }

    #[test]
    fn minify_keeps_word_separation_in_text() {
        assert_eq!(minify_html("<p>hello\n  world</p>"), "<p>hello world</p>");
    }

    #[test]
    fn slugify_keeps_cjk_and_collapses_separators() {
        assert_eq!(slugify_name("Hello World"), "hello-world");
        assert_eq!(slugify_name("统一模板 与 级联更新"), "统一模板-与-级联更新");
        assert_eq!(slugify_name("a//b??c"), "a-b-c");
        assert_eq!(slugify_name("__keep_underscores__"), "__keep_underscores__");
        assert_eq!(slugify_name("???"), "");
    }

    #[test]
    fn slugify_is_length_bounded() {
        assert!(slugify_name(&"a".repeat(200)).chars().count() <= 64);
    }

    #[test]
    fn sanitize_relative_dir_drops_traversal() {
        assert_eq!(sanitize_relative_dir("posts"), "posts");
        assert_eq!(sanitize_relative_dir("/posts/2026/"), "posts/2026");
        assert_eq!(sanitize_relative_dir("..\\..\\etc"), "etc");
        assert_eq!(sanitize_relative_dir("./a/./b"), "a/b");
        assert_eq!(sanitize_relative_dir(""), "");
    }
}
