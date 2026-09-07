//! 标签聚合（taxonomy）。
//!
//! front matter 里的 `tags` 之前只在文章页显示，没有任何页面用到它们——
//! 这里把它们聚合成「标签总览」与「单个标签」两级页面。
//!
//! 聚合是纯函数，因此排序规则、slug 冲突处理都能单测，不必跑一次完整构建。

use serde::Serialize;

use crate::content::Page;
use crate::util::slugify_name;

/// 一个标签。传给模板的形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Term {
    pub name: String,
    /// URL 片段，冲突时带序号后缀。
    pub slug: String,
    /// 站内地址，如 `/tags/rust/`。
    pub url: String,
    pub count: usize,
}

/// 标签 + 属于它的文章。
#[derive(Debug, Clone)]
pub struct TermPages<'a> {
    pub term: Term,
    pub pages: Vec<&'a Page>,
}

/// 聚合某个维度的词条。
///
/// - `field` 是 front matter 字段名（`tags` / `categories` / 自定义）
/// - 只统计非栏目索引页（索引页的分类没有意义）
/// - 词条按文章数降序、同数按名称升序，方便直接渲染成标签云
/// - 每个词条内部按 `weight` 升序、日期降序，与栏目列表页一致
pub fn collect<'a>(pages: &[&'a Page], base_slug: &str, field: &str) -> Vec<TermPages<'a>> {
    let mut grouped: Vec<(String, Vec<&'a Page>)> = Vec::new();

    for page in pages.iter().filter(|p| !p.is_index) {
        let Some(values) = page.taxonomies.get(field) else {
            continue;
        };
        for tag in values {
            let name = tag.trim();
            if name.is_empty() {
                continue;
            }
            match grouped.iter_mut().find(|(existing, _)| existing == name) {
                Some((_, list)) => list.push(page),
                None => grouped.push((name.to_string(), vec![page])),
            }
        }
    }

    grouped.sort_by(|(a_name, a_pages), (b_name, b_pages)| {
        b_pages.len().cmp(&a_pages.len()).then(a_name.cmp(b_name))
    });

    let mut used: Vec<String> = Vec::new();
    grouped
        .into_iter()
        .map(|(name, mut pages)| {
            pages.sort_by(|a, b| {
                a.weight
                    .cmp(&b.weight)
                    .then_with(|| b.date.cmp(&a.date))
                    .then_with(|| a.title.cmp(&b.title))
            });
            let slug = unique_slug(&name, &mut used);
            let prefix = base_slug.trim_matches('/');
            Term {
                url: format!("/{prefix}/{slug}/"),
                count: pages.len(),
                name,
                slug,
            }
            .pair(pages)
        })
        .collect()
}

impl Term {
    fn pair<'a>(self, pages: Vec<&'a Page>) -> TermPages<'a> {
        TermPages { term: self, pages }
    }
}

/// 生成不重复的 slug。
///
/// 不同标签可能清洗出同样的 slug（`C++` 与 `C  ++`），冲突时追加序号，
/// 否则两个标签会写到同一个文件、后者覆盖前者。
fn unique_slug(name: &str, used: &mut Vec<String>) -> String {
    let base = {
        let cleaned = slugify_name(name);
        if cleaned.is_empty() {
            "tag".to_string()
        } else {
            cleaned
        }
    };
    let mut candidate = base.clone();
    let mut suffix = 2;
    while used.contains(&candidate) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    used.push(candidate.clone());
    candidate
}

/// 只要词条本身（给「标签总览」页用）。
pub fn terms_of(collected: &[TermPages<'_>]) -> Vec<Term> {
    collected.iter().map(|t| t.term.clone()).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn page(source: &str, raw: &str) -> Page {
        Page::from_str(
            Path::new("/site/content"),
            &Path::new("/site/content").join(source),
            raw,
        )
        .unwrap()
    }

    fn tagged(source: &str, title: &str, date: &str, tags: &[&str]) -> Page {
        let list = tags
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        page(
            source,
            &format!("+++\ntitle = \"{title}\"\ndate = \"{date}\"\ntags = [{list}]\n+++\n"),
        )
    }

    #[test]
    fn groups_pages_by_tag_and_sorts_by_popularity() {
        let a = tagged("posts/a.md", "A", "2026-09-01", &["rust", "模板"]);
        let b = tagged("posts/b.md", "B", "2026-09-02", &["rust"]);
        let c = tagged("posts/c.md", "C", "2026-09-03", &["模板"]);
        let pages = [&a, &b, &c];

        let collected = collect(&pages, "tags", "tags");
        let names: Vec<&str> = collected.iter().map(|t| t.term.name.as_str()).collect();

        // rust 与 模板 都是 2 篇，按名称升序（中文按码位排在字母后）
        assert_eq!(names, vec!["rust", "模板"]);
        assert_eq!(collected[0].term.count, 2);
        assert_eq!(collected[0].term.url, "/tags/rust/");
    }

    #[test]
    fn pages_inside_a_term_are_newest_first() {
        let old = tagged("posts/old.md", "旧", "2026-01-01", &["rust"]);
        let new = tagged("posts/new.md", "新", "2026-09-01", &["rust"]);
        let pages = [&old, &new];

        let collected = collect(&pages, "tags", "tags");
        let titles: Vec<&str> = collected[0]
            .pages
            .iter()
            .map(|p| p.title.as_str())
            .collect();
        assert_eq!(titles, vec!["新", "旧"]);
    }

    #[test]
    fn section_index_pages_are_excluded() {
        let index = page(
            "posts/index.md",
            "+++\ntitle = \"归档\"\ntags = [\"rust\"]\n+++\n",
        );
        let post = tagged("posts/a.md", "A", "2026-09-01", &["rust"]);
        let pages = [&index, &post];

        let collected = collect(&pages, "tags", "tags");
        assert_eq!(collected[0].term.count, 1);
        assert_eq!(collected[0].pages[0].title, "A");
    }

    #[test]
    fn empty_and_whitespace_tags_are_ignored() {
        let p = tagged("posts/a.md", "A", "2026-09-01", &["  ", "rust"]);
        let collected = collect(&[&p], "tags", "tags");
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].term.name, "rust");
    }

    #[test]
    fn tags_that_clean_to_the_same_slug_get_distinct_urls() {
        let a = tagged("posts/a.md", "A", "2026-09-01", &["C++"]);
        let b = tagged("posts/b.md", "B", "2026-09-02", &["C  ++"]);
        let pages = [&a, &b];

        let collected = collect(&pages, "tags", "tags");
        assert_eq!(collected.len(), 2);
        let slugs: Vec<&str> = collected.iter().map(|t| t.term.slug.as_str()).collect();
        assert_ne!(slugs[0], slugs[1], "slug 冲突必须区分，否则文件互相覆盖");
    }

    #[test]
    fn cjk_tags_keep_their_characters_in_urls() {
        let p = tagged("posts/a.md", "A", "2026-09-01", &["增量构建"]);
        let collected = collect(&[&p], "tags", "tags");
        assert_eq!(collected[0].term.url, "/tags/增量构建/");
    }

    #[test]
    fn base_slug_is_normalized() {
        let p = tagged("posts/a.md", "A", "2026-09-01", &["rust"]);
        let collected = collect(&[&p], "/topics/", "tags");
        assert_eq!(collected[0].term.url, "/topics/rust/");
    }

    #[test]
    fn terms_of_extracts_the_template_shape() {
        let p = tagged("posts/a.md", "A", "2026-09-01", &["rust"]);
        let collected = collect(&[&p], "tags", "tags");
        let terms = terms_of(&collected);
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].name, "rust");
    }

    #[test]
    fn no_tags_yields_nothing() {
        let p = page("posts/a.md", "+++\ntitle = \"A\"\n+++\n");
        assert!(collect(&[&p], "tags", "tags").is_empty());
    }
}
