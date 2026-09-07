use std::collections::HashMap;

use tera::{Tera, Value};

/// 注册 StaticSmith 内置的 Tera 扩展。
///
/// Tera 自带 `now()`、`slugify`、`truncate`、`date` 等；这里只补充静态站点特有的能力。
pub fn register(tera: &mut Tera) {
    tera.register_filter("markdown", markdown_filter);
    tera.register_filter("absolute_url", absolute_url_filter);
}

/// `{{ page.extra.note | markdown }}`：把 front matter 里的 Markdown 片段渲染为 HTML。
fn markdown_filter(value: &Value, _args: &HashMap<String, Value>) -> tera::Result<Value> {
    let text = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("markdown 过滤器只接受字符串"))?;
    Ok(Value::String(crate::content::markdown_to_html(text)))
}

/// `{{ "/about/" | absolute_url(base=site.base_url) }}`：拼接站点绝对地址，避免重复斜杠。
fn absolute_url_filter(value: &Value, args: &HashMap<String, Value>) -> tera::Result<Value> {
    let path = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("absolute_url 过滤器只接受字符串"))?;
    let base = args
        .get("base")
        .and_then(|v| v.as_str())
        .ok_or_else(|| tera::Error::msg("absolute_url 需要 base 参数"))?;
    Ok(Value::String(join_url(base, path)))
}

pub fn join_url(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_normalizes_slashes() {
        assert_eq!(
            join_url("https://a.com/", "/about/"),
            "https://a.com/about/"
        );
        assert_eq!(join_url("https://a.com", "about/"), "https://a.com/about/");
    }

    #[test]
    fn join_url_passes_through_absolute_links() {
        assert_eq!(
            join_url("https://a.com", "https://b.com/x"),
            "https://b.com/x"
        );
    }

    #[test]
    fn markdown_filter_renders_inline_markdown() {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", "{{ text | markdown }}").unwrap();
        let mut ctx = tera::Context::new();
        ctx.insert("text", "**粗**");
        assert!(tera.render("t", &ctx).unwrap().contains("<strong>"));
    }

    #[test]
    fn absolute_url_filter_requires_base() {
        let mut tera = Tera::default();
        register(&mut tera);
        tera.add_raw_template("t", "{{ \"/a/\" | absolute_url }}")
            .unwrap();
        assert!(tera.render("t", &tera::Context::new()).is_err());
    }
}
