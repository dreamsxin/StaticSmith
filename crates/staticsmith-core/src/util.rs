use std::path::Path;

use sha2::{Digest, Sha256};

/// 统一使用正斜杠表示相对路径：Windows 与 macOS 的索引键、模板名必须一致。
pub fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
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

fn hex16(digest: &[u8]) -> String {
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
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
}
