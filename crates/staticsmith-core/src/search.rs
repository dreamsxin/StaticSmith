//! 全文搜索：在标题与正文里找一个词，返回带上下文的片段。
//!
//! 为什么放在 core 而不是各端各写一份：桌面端侧栏、MCP 的 `search_content`、
//! 以后可能加的 CLI `search` 问的是同一个问题「哪几篇提到过这个词」。
//! 判断一样、片段一样，用户在界面里搜到的和 Agent 搜到的才会是同一批。
//!
//! 两条刻意的取舍：
//!
//! - **不做分词与打分。** 中文标题里子串匹配已经足够准，而一个排序玄学的结果列表
//!   会让人怀疑「是不是漏了」。命中就按站点自己的页面顺序给出来。
//! - **搜的是「读者看到的文字」。** `Page::content` 是渲染后的 HTML，直接拿它当草堆
//!   会命中标签名与属性（搜 `img` 能搜出每一张图片），也会因为实体转义
//!   （`&amp;`、`&#x2F;`）搜不到原文里真的有的字符。所以先还原成纯文本再搜。
use serde::{Deserialize, Serialize};

use crate::content::Page;

/// 命中位置：标题还是正文。界面据此决定要不要显示片段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Title,
    Body,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hit {
    pub source: String,
    pub title: String,
    pub url: String,
    /// 命中处的上下文，**保留原文大小写**，两端按需要加省略号。
    pub snippet: String,
    pub field: Field,
}

/// 片段在命中词前面留多少个字符。
const LEAD_CHARS: usize = 24;
/// 片段总长度（字符数，不是字节）。
const SNIPPET_CHARS: usize = 120;

/// 搜索。`query` 大小写不敏感；空白查询返回空结果（而不是全站）。
///
/// `limit` 是命中条数上限，0 视为 1：返回空列表会让调用方以为「没搜到」。
pub fn search<'a, I>(pages: I, query: &str, limit: usize) -> Vec<Hit>
where
    I: IntoIterator<Item = &'a Page>,
{
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let limit = limit.max(1);

    let mut hits = Vec::new();
    for page in pages {
        // 标题优先：命中标题时片段给整个标题，比截一段正文更说明问题
        if find_ignore_case(&page.title, &needle).is_some() {
            hits.push(Hit {
                source: page.source.clone(),
                title: page.title.clone(),
                url: page.url.clone(),
                snippet: page.title.clone(),
                field: Field::Title,
            });
        } else {
            let body = plain_text(&page.content);
            if let Some(at) = find_ignore_case(&body, &needle) {
                hits.push(Hit {
                    source: page.source.clone(),
                    title: page.title.clone(),
                    url: page.url.clone(),
                    snippet: snippet_around(&body, at),
                    field: Field::Body,
                });
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

/// 大小写不敏感地找第一处命中，返回**原文**里的字节偏移。
///
/// 不能直接在 `to_lowercase()` 的结果里找完了拿偏移用：小写化会改变字节长度
/// （`İ` 一个字符会变成两个），偏移对不上就会在别处切片，非 ASCII 站点上必然崩。
/// 所以边小写化边记下每个字节来自原文的哪个位置。
fn find_ignore_case(text: &str, needle_lower: &str) -> Option<usize> {
    let mut lower = String::with_capacity(text.len());
    // map[i] = lower 的第 i 个字节对应原文的字节偏移
    let mut map: Vec<usize> = Vec::with_capacity(text.len() + 1);
    for (at, ch) in text.char_indices() {
        for lc in ch.to_lowercase() {
            lower.push(lc);
            map.resize(lower.len(), at);
        }
    }
    map.push(text.len());
    lower.find(needle_lower).map(|at| map[at])
}

/// 取命中处前后的一段，两端按需要加省略号。
fn snippet_around(text: &str, at: usize) -> String {
    let start = text[..at]
        .char_indices()
        .rev()
        .nth(LEAD_CHARS.saturating_sub(1))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let rest = &text[start..];
    let mut out: String = rest.chars().take(SNIPPET_CHARS).collect();
    let truncated = rest.chars().count() > SNIPPET_CHARS;
    if start > 0 {
        out.insert(0, '…');
    }
    if truncated {
        out.push('…');
    }
    out
}

/// 把渲染后的 HTML 还原成纯文本：去标签、还原实体、把连续空白压成一个空格。
///
/// 只处理 Markdown 渲染会产出的那几种实体。这里不追求通用 HTML 解析——
/// 草堆是自己渲染出来的，输入范围可控。
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut chars = html.char_indices().peekable();

    while let Some((at, ch)) = chars.next() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                // 标签之间补一个空格，否则 `<p>甲</p><p>乙</p>` 会连成「甲乙」，
                // 搜「甲乙」反而能搜到一个原文里不存在的词
                push_space(&mut out);
            }
            _ if in_tag => {}
            '&' => {
                if let Some((entity, consumed)) = read_entity(&html[at..]) {
                    out.push(entity);
                    for _ in 1..consumed {
                        chars.next();
                    }
                } else {
                    out.push('&');
                }
            }
            c if c.is_whitespace() => push_space(&mut out),
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

fn push_space(out: &mut String) {
    if !out.ends_with(' ') && !out.is_empty() {
        out.push(' ');
    }
}

/// 读一个 HTML 实体，返回（字符，消耗的**字符数**）。
fn read_entity(rest: &str) -> Option<(char, usize)> {
    const NAMED: [(&str, char); 5] = [
        ("&amp;", '&'),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&quot;", '"'),
        ("&#39;", '\''),
    ];
    for (name, ch) in NAMED {
        if rest.starts_with(name) {
            return Some((ch, name.chars().count()));
        }
    }
    // 数字实体：Tera 的自动转义会把 `/` 写成 &#x2F;
    let end = rest.find(';')?;
    let body = &rest[1..end];
    let code = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        body.strip_prefix('#')?.parse::<u32>().ok()?
    };
    char::from_u32(code).map(|ch| (ch, rest[..=end].chars().count()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(source: &str, title: &str, content: &str) -> Page {
        Page {
            source: source.to_string(),
            output: String::new(),
            url: format!("/{source}/"),
            slug: String::new(),
            title: title.to_string(),
            description: String::new(),
            date: None,
            tags: Vec::new(),
            keywords: Vec::new(),
            taxonomies: Default::default(),
            aliases: Vec::new(),
            draft: false,
            weight: 0,
            section: String::new(),
            is_index: false,
            template: String::new(),
            content: content.to_string(),
            hash: String::new(),
            extra: Default::default(),
        }
    }

    #[test]
    fn matches_title_case_insensitively() {
        let pages = vec![page("a.md", "Hello World", "<p>正文</p>")];
        let hits = search(&pages, "hello", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].field, Field::Title);
        // 片段保留原文大小写
        assert_eq!(hits[0].snippet, "Hello World");
    }

    #[test]
    fn searches_plain_text_not_markup() {
        let pages = vec![page("a.md", "标题", "<p><img src=\"/x.png\"> 一段正文</p>")];
        // 标签名不该被搜到，否则搜 img 会把每一张带图的文章都搜出来
        assert!(search(&pages, "img", 10).is_empty());
        assert_eq!(search(&pages, "一段正文", 10).len(), 1);
    }

    #[test]
    fn decodes_entities_so_source_text_is_findable() {
        let pages = vec![page(
            "a.md",
            "标题",
            "<p>a &amp; b 和 &#x2F;posts&#x2F;</p>",
        )];
        assert_eq!(search(&pages, "a & b", 10).len(), 1);
        assert_eq!(search(&pages, "/posts/", 10).len(), 1);
    }

    #[test]
    fn does_not_join_adjacent_blocks() {
        let pages = vec![page("a.md", "标题", "<p>甲</p><p>乙</p>")];
        // 「甲乙」在原文里并不存在，不该命中
        assert!(search(&pages, "甲乙", 10).is_empty());
        assert_eq!(search(&pages, "甲", 10).len(), 1);
    }

    #[test]
    fn snippet_marks_truncation_and_keeps_char_boundaries() {
        let long = format!("<p>{}命中词{}</p>", "前".repeat(60), "后".repeat(200));
        let pages = vec![page("a.md", "标题", &long)];
        let hits = search(&pages, "命中词", 10);
        assert_eq!(hits.len(), 1);
        let snippet = &hits[0].snippet;
        assert!(snippet.starts_with('…'), "左侧截断要有省略号: {snippet}");
        assert!(snippet.ends_with('…'), "右侧截断要有省略号: {snippet}");
        assert!(snippet.contains("命中词"));
    }

    #[test]
    fn empty_query_matches_nothing() {
        let pages = vec![page("a.md", "标题", "<p>正文</p>")];
        assert!(search(&pages, "   ", 10).is_empty());
    }

    #[test]
    fn limit_is_respected_and_never_zero() {
        let pages = vec![
            page("a.md", "同一个词", "<p>x</p>"),
            page("b.md", "同一个词", "<p>x</p>"),
        ];
        assert_eq!(search(&pages, "同一个词", 1).len(), 1);
        // limit 0 当 1 用：返回空列表会被读成「没搜到」
        assert_eq!(search(&pages, "同一个词", 0).len(), 1);
    }
}
