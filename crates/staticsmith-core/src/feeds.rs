//! 站点级 XML 产物：`sitemap.xml` 与 Atom 订阅。
//!
//! 两者都需要绝对地址，因此 `site.base_url` 为空时会被跳过（构建报告里给出 warning），
//! 而不是生成一份带相对链接的、搜索引擎与阅读器都不认的文件。

use crate::config::SiteConfig;
use crate::content::Page;
use crate::filters::join_url;

/// 生成 `sitemap.xml`。
pub fn sitemap_xml(base_url: &str, pages: &[&Page]) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for page in pages {
        out.push_str("  <url>\n");
        out.push_str(&format!(
            "    <loc>{}</loc>\n",
            escape(&join_url(base_url, &page.url))
        ));
        if let Some(date) = page.date {
            out.push_str(&format!(
                "    <lastmod>{}</lastmod>\n",
                date.format("%Y-%m-%d")
            ));
        }
        out.push_str("  </url>\n");
    }
    out.push_str("</urlset>\n");
    out
}

/// 生成 Atom 1.0 订阅。
///
/// 选 Atom 而不是 RSS 2.0：时间格式是明确的 RFC3339，`id` 语义清晰，
/// 现代阅读器全部支持。`limit` 为 0 时输出全部条目。
pub fn atom_xml(config: &SiteConfig, pages: &[&Page], limit: usize, updated: &str) -> String {
    let base = &config.site.base_url;
    let feed_url = join_url(base, "/feed.xml");

    let mut entries: Vec<&&Page> = pages.iter().filter(|p| !p.is_index).collect();
    entries.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.title.cmp(&b.title)));
    if limit > 0 {
        entries.truncate(limit);
    }

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<feed xmlns=\"http://www.w3.org/2005/Atom\" xml:lang=\"");
    out.push_str(&escape(&config.site.language));
    out.push_str("\">\n");
    out.push_str(&format!(
        "  <title>{}</title>\n",
        escape(&config.site.title)
    ));
    if !config.site.description.is_empty() {
        out.push_str(&format!(
            "  <subtitle>{}</subtitle>\n",
            escape(&config.site.description)
        ));
    }
    out.push_str(&format!("  <id>{}</id>\n", escape(&join_url(base, "/"))));
    out.push_str(&format!("  <updated>{}</updated>\n", escape(updated)));
    out.push_str(&format!(
        "  <link rel=\"alternate\" href=\"{}\"/>\n",
        escape(&join_url(base, "/"))
    ));
    out.push_str(&format!(
        "  <link rel=\"self\" href=\"{}\"/>\n",
        escape(&feed_url)
    ));
    out.push_str(
        "  <generator uri=\"https://github.com/dreamsxin/StaticSmith\">StaticSmith</generator>\n",
    );

    for page in entries {
        let link = join_url(base, &page.url);
        let stamp = page
            .date
            .map(|d| d.to_rfc3339())
            .unwrap_or_else(|| updated.to_string());
        out.push_str("  <entry>\n");
        out.push_str(&format!("    <title>{}</title>\n", escape(&page.title)));
        out.push_str(&format!("    <id>{}</id>\n", escape(&link)));
        out.push_str(&format!(
            "    <link rel=\"alternate\" href=\"{}\"/>\n",
            escape(&link)
        ));
        out.push_str(&format!("    <updated>{}</updated>\n", escape(&stamp)));
        if !page.description.is_empty() {
            out.push_str(&format!(
                "    <summary>{}</summary>\n",
                escape(&page.description)
            ));
        }
        for tag in &page.tags {
            out.push_str(&format!("    <category term=\"{}\"/>\n", escape(tag)));
        }
        // 正文是 HTML，按 Atom 规范转义后放进 type="html"。
        out.push_str(&format!(
            "    <content type=\"html\">{}</content>\n",
            escape(&page.content)
        ));
        out.push_str("  </entry>\n");
    }

    out.push_str("</feed>\n");
    out
}

/// XML 文本转义。顺序重要：`&` 必须先处理。
fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::content::Page;

    fn page(source: &str, raw: &str) -> Page {
        Page::from_str(
            Path::new("/site/content"),
            &Path::new("/site/content").join(source),
            raw,
        )
        .unwrap()
    }

    fn config() -> SiteConfig {
        let mut config = SiteConfig::default();
        config.site.title = "我的站点".into();
        config.site.description = "描述 & 说明".into();
        config.site.base_url = "https://example.com/".into();
        config.site.language = "zh-CN".into();
        config
    }

    #[test]
    fn sitemap_lists_absolute_urls_with_lastmod() {
        let a = page("index.md", "+++\ntitle=\"首页\"\n+++\n");
        let b = page(
            "posts/hello.md",
            "+++\ntitle=\"文章\"\ndate=\"2026-09-07\"\n+++\n",
        );
        let xml = sitemap_xml("https://example.com/", &[&a, &b]);

        assert!(xml.contains("<loc>https://example.com/</loc>"));
        assert!(xml.contains("<loc>https://example.com/posts/hello/</loc>"));
        assert!(xml.contains("<lastmod>2026-09-07</lastmod>"));
        // 首页没有日期，不应输出空的 lastmod。
        assert_eq!(xml.matches("<lastmod>").count(), 1);
    }

    #[test]
    fn atom_sorts_newest_first_and_skips_section_indexes() {
        let index = page("posts/index.md", "+++\ntitle=\"归档\"\n+++\n");
        let old = page(
            "posts/a.md",
            "+++\ntitle=\"旧\"\ndate=\"2026-01-01\"\n+++\n",
        );
        let new = page(
            "posts/b.md",
            "+++\ntitle=\"新\"\ndate=\"2026-09-07\"\n+++\n",
        );

        let xml = atom_xml(&config(), &[&index, &old, &new], 0, "2026-09-07T00:00:00Z");

        let first = xml.find("<title>新</title>").unwrap();
        let second = xml.find("<title>旧</title>").unwrap();
        assert!(first < second, "新条目应在前");
        assert!(!xml.contains("<title>归档</title>"), "栏目索引页不进订阅");
        assert!(xml.contains("rel=\"self\" href=\"https://example.com/feed.xml\""));
    }

    #[test]
    fn atom_respects_the_limit() {
        let pages: Vec<Page> = (1..=5)
            .map(|i| {
                page(
                    &format!("posts/p{i}.md"),
                    &format!("+++\ntitle=\"文章 {i}\"\ndate=\"2026-09-0{i}\"\n+++\n"),
                )
            })
            .collect();
        let refs: Vec<&Page> = pages.iter().collect();

        let xml = atom_xml(&config(), &refs, 2, "2026-09-07T00:00:00Z");
        assert_eq!(xml.matches("<entry>").count(), 2);
    }

    #[test]
    fn xml_special_characters_are_escaped() {
        let p = page(
            "posts/x.md",
            "+++\ntitle=\"A & B <c>\"\n+++\n正文 <script>alert(1)</script>\n",
        );
        let xml = atom_xml(&config(), &[&p], 0, "2026-09-07T00:00:00Z");

        assert!(xml.contains("<title>A &amp; B &lt;c&gt;</title>"));
        assert!(xml.contains("&lt;script&gt;"));
        assert!(!xml.contains("<script>"), "正文 HTML 必须转义进 content");
        assert!(xml.contains("描述 &amp; 说明"));
    }

    #[test]
    fn tags_become_categories() {
        let p = page(
            "posts/x.md",
            "+++\ntitle=\"t\"\ntags=[\"模板\",\"增量\"]\n+++\n",
        );
        let xml = atom_xml(&config(), &[&p], 0, "2026-09-07T00:00:00Z");
        assert!(xml.contains("<category term=\"模板\"/>"));
        assert!(xml.contains("<category term=\"增量\"/>"));
    }
}
