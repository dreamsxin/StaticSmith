//! 导航菜单的校验。
//!
//! 与前端 `src/menu.ts` 是一对**孪生实现**：同一套规则要在两个时机生效——
//! 编辑菜单时立刻提示（前端，不走 IPC 才谈得上「立刻」），生成时再报一次
//! （这里，CLI 与 MCP 也就都有了）。两边各有测试钉同样的例子，改一边就得改另一边。
//!
//! 为什么值得做两遍：菜单渲染在**每一页**的头部，一个错地址就是全站死链，
//! 而死链原先只能等 [`crate::links`] 的体检发现——那已经是生成之后的事。

use std::collections::BTreeSet;

use crate::config::MenuItem;

/// 比较地址时先规整：**末尾斜杠与锚点都不算差异**。
///
/// 产物里 `/about` 与 `/about/` 打开的是同一个 `index.html`（[`crate::links`] 的死链
/// 体检也是这么宽容的），把它们当两个地址会报出一堆假问题。`/about/#history`
/// 指的还是 `/about/`。
pub fn normalize_url(url: &str) -> &str {
    let without_hash = url.split('#').next().unwrap_or("").trim();
    if without_hash.len() > 1 {
        without_hash.trim_end_matches('/')
    } else {
        without_hash
    }
}

/// 菜单里指向站内、但站内没有这个地址的那些项。
///
/// 只查**站内**的：站外链接与纯锚点的正确性不在这个站里。空地址也不查——
/// 那是 [`MenuItem::validate`] 的活儿，在这里再报一遍只是噪音。
///
/// `known` 收的是**产物地址**（每个已发布页面的 `url`，包括栏目索引页）。
/// 没有索引页的栏目因此自然不在其中——它的地址本来就是 404。
pub fn unknown_urls<'a>(menu: &'a [MenuItem], known: &BTreeSet<String>) -> Vec<&'a str> {
    let mut seen = BTreeSet::new();
    let mut bad = Vec::new();
    for item in menu {
        let url = item.url.trim();
        if url.is_empty() || item.is_external() || url.starts_with('#') {
            continue;
        }
        if !known.contains(normalize_url(url)) && seen.insert(normalize_url(url)) {
            bad.push(item.url.as_str());
        }
    }
    bad
}

/// 这条警告的措辞只有这一份（界面、CLI、MCP 共用同一句）。
///
/// 说清「在哪儿」和「会怎样」：用户看到的是构建报告，而菜单在每一页上，
/// 不点明「每一页」他会以为只是某一处小毛病。
pub fn unknown_urls_warning(bad: &[&str]) -> String {
    format!(
        "导航菜单里有 {} 个地址在站内找不到（{}）——它们出现在每一页的头部，都是死链",
        bad.len(),
        bad.join("、")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(url: &str) -> MenuItem {
        MenuItem {
            name: "项".into(),
            url: url.into(),
            weight: 0,
            blank: false,
        }
    }

    fn known() -> BTreeSet<String> {
        ["/", "/posts", "/about"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn reports_only_urls_the_site_does_not_have() {
        let menu = vec![item("/posts/"), item("/abuot/")];
        assert_eq!(unknown_urls(&menu, &known()), vec!["/abuot/"]);
    }

    #[test]
    fn external_links_and_anchors_are_not_our_business() {
        let menu = vec![
            item("https://example.com/"),
            item("//cdn.example.com/x"),
            item("mailto:a@b.c"),
            item("#top"),
            item("/about/#history"),
        ];
        assert!(unknown_urls(&menu, &known()).is_empty());
    }

    #[test]
    fn trailing_slash_is_not_a_difference() {
        let menu = vec![item("/about"), item("/about/")];
        assert!(unknown_urls(&menu, &known()).is_empty());
    }

    #[test]
    fn empty_url_is_left_to_validate() {
        let menu = vec![item(""), item("   ")];
        assert!(unknown_urls(&menu, &known()).is_empty());
    }

    #[test]
    fn the_same_bad_url_is_reported_once() {
        let menu = vec![item("/nope/"), item("/nope")];
        assert_eq!(unknown_urls(&menu, &known()), vec!["/nope/"]);
    }

    /// 措辞里必须点明「每一页」，否则会被当成某一处的小毛病。
    #[test]
    fn warning_says_where_it_hurts() {
        let text = unknown_urls_warning(&["/a/", "/b/"]);
        assert!(text.contains("2 个"));
        assert!(text.contains("/a/、/b/"));
        assert!(text.contains("每一页"));
    }
}
