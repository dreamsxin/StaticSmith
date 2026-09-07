//! SEO 体检：把「这篇文章还差什么才算能上线」变成一份可执行清单。
//!
//! 定位是内容运营的日常工具，不是学术评分器，所以规则只保留能直接动手改的项，
//! 每条都带上源文件路径与具体建议——AI Agent 也照这份清单补字段（见 `docs/seo.md`）。
//!
//! 阈值取自各家搜索引擎与社交卡片的常见截断位置，写在 [`Thresholds`] 里而不是散在判断中，
//! 便于站点按自己的语言习惯调整（中文一个字占的宽度约等于两个英文字符）。

use serde::Serialize;

use crate::config::SiteConfig;
use crate::content::Page;

/// 问题严重程度。界面按此排序与配色，运营先处理 error。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 会直接影响收录或渲染，必须改
    Error,
    /// 会影响搜索结果里的展示效果，应该改
    Warn,
    /// 锦上添花
    Hint,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "必须修",
            Severity::Warn => "建议修",
            Severity::Hint => "可优化",
        }
    }
}

/// 一条体检结论。`source` 为空表示站点级问题（不属于某一篇）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub source: String,
    pub url: String,
    pub title: String,
    pub severity: Severity,
    /// 稳定的规则标识，便于将来支持忽略清单
    pub code: String,
    pub message: String,
}

/// 体检报告。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// 参与体检的页面数（草稿不算，它们不会发布）
    pub checked: usize,
    pub errors: usize,
    pub warnings: usize,
    pub hints: usize,
    /// 0-100 的粗略健康度，只用于「今天比昨天好没好」的对比
    pub score: u8,
    pub issues: Vec<Issue>,
}

/// 长度阈值，单位为字符（中文按字计）。
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub title_max: usize,
    pub description_min: usize,
    pub description_max: usize,
    pub content_min: usize,
}

impl Default for Thresholds {
    fn default() -> Self {
        // 标题 60、描述 160 是桌面搜索结果的常见截断位置；
        // 描述短于 40 基本等于没写，正文短于 200 字很难被判定为有价值内容。
        Self {
            title_max: 60,
            description_min: 40,
            description_max: 160,
            content_min: 200,
        }
    }
}

/// 用默认阈值体检。
pub fn audit(pages: &[Page], config: &SiteConfig) -> Report {
    audit_with(pages, config, Thresholds::default())
}

/// 体检。草稿与未到发布时间的文章被跳过：它们不进产物，报了只会淹没真正要改的条目。
pub fn audit_with(pages: &[Page], config: &SiteConfig, limits: Thresholds) -> Report {
    let now = chrono::Utc::now();
    let published: Vec<&Page> = pages
        .iter()
        .filter(|p| p.is_publishable() && p.is_released_at(now, config.build.publish_future))
        .collect();
    let mut issues = Vec::new();

    if config.site.base_url.trim().is_empty() {
        issues.push(Issue {
            source: String::new(),
            url: String::new(),
            title: config.site.title.clone(),
            severity: Severity::Warn,
            code: "site.base_url_missing".into(),
            message: "站点未配置 base_url，canonical、Open Graph 与 sitemap 里的绝对地址都无法生成"
                .into(),
        });
    }
    if config.site.description.trim().is_empty() {
        issues.push(Issue {
            source: String::new(),
            url: String::new(),
            title: config.site.title.clone(),
            severity: Severity::Hint,
            code: "site.description_missing".into(),
            message: "站点未配置 description，没有自己描述的页面会连兜底文案都没有".into(),
        });
    }

    for page in &published {
        let mut push = |severity: Severity, code: &str, message: String| {
            issues.push(Issue {
                source: page.source.clone(),
                url: page.url.clone(),
                title: page.title.clone(),
                severity,
                code: code.to_string(),
                message,
            });
        };

        let title_len = page.title.chars().count();
        if page.title.trim().is_empty() {
            push(
                Severity::Error,
                "title.missing",
                "缺少标题：front matter 的 title 为空，正文里也没有一级标题".into(),
            );
        } else if title_len > limits.title_max {
            push(
                Severity::Warn,
                "title.too_long",
                format!(
                    "标题 {title_len} 字，超过 {} 字后会在搜索结果里被截断",
                    limits.title_max
                ),
            );
        }

        let desc_len = page.description.chars().count();
        if page.description.trim().is_empty() {
            push(
                Severity::Warn,
                "description.missing",
                "缺少描述：搜索结果与订阅源会退回站点描述或正文摘要".into(),
            );
        } else if desc_len < limits.description_min {
            push(
                Severity::Hint,
                "description.too_short",
                format!(
                    "描述只有 {desc_len} 字，建议写到 {}-{} 字",
                    limits.description_min, limits.description_max
                ),
            );
        } else if desc_len > limits.description_max {
            push(
                Severity::Warn,
                "description.too_long",
                format!(
                    "描述 {desc_len} 字，超过 {} 字会被截断",
                    limits.description_max
                ),
            );
        }

        if page.keywords.is_empty() {
            push(
                Severity::Hint,
                "keywords.missing",
                "没有关键词也没有标签，meta keywords 与标签页都会缺这一篇".into(),
            );
        }

        let body_len = plain_text_len(&page.content);
        if body_len < limits.content_min {
            push(
                Severity::Hint,
                "content.too_short",
                format!(
                    "正文约 {body_len} 字，短于 {} 字的页面很难被判定为有价值内容",
                    limits.content_min
                ),
            );
        }
    }

    issues.extend(duplicates(
        &published,
        "title.duplicated",
        |p| p.title.trim().to_lowercase(),
        "标题与其他页面重复",
    ));
    issues.extend(duplicates(
        &published,
        "description.duplicated",
        |p| p.description.trim().to_lowercase(),
        "描述与其他页面重复",
    ));

    // 站点级问题排在最前，其余按严重程度再按路径，界面直接照序渲染
    issues.sort_by(|a, b| {
        a.source
            .is_empty()
            .cmp(&b.source.is_empty())
            .reverse()
            .then(a.severity.cmp(&b.severity))
            .then(a.source.cmp(&b.source))
            .then(a.code.cmp(&b.code))
    });

    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|i| i.severity == Severity::Warn)
        .count();
    let hints = issues
        .iter()
        .filter(|i| i.severity == Severity::Hint)
        .count();

    Report {
        checked: published.len(),
        errors,
        warnings,
        hints,
        score: score(errors, warnings, hints),
        issues,
    }
}

/// 找出取值重复的页面。空值不算重复——那是「缺失」，另有规则。
fn duplicates(
    pages: &[&Page],
    code: &str,
    key: impl Fn(&Page) -> String,
    message: &str,
) -> Vec<Issue> {
    use std::collections::BTreeMap;

    let mut buckets: BTreeMap<String, Vec<&Page>> = BTreeMap::new();
    for page in pages {
        let value = key(page);
        if value.is_empty() {
            continue;
        }
        buckets.entry(value).or_default().push(page);
    }

    let mut issues = Vec::new();
    for (_, group) in buckets.into_iter().filter(|(_, g)| g.len() > 1) {
        let others: Vec<&str> = group.iter().map(|p| p.source.as_str()).collect();
        for page in &group {
            let peers: Vec<&str> = others
                .iter()
                .copied()
                .filter(|s| *s != page.source.as_str())
                .collect();
            issues.push(Issue {
                source: page.source.clone(),
                url: page.url.clone(),
                title: page.title.clone(),
                severity: Severity::Warn,
                code: code.to_string(),
                message: format!("{message}：{}", peers.join("、")),
            });
        }
    }
    issues
}

/// 粗略健康度。权重按「改起来的紧迫程度」排，不追求与任何第三方评分对齐。
fn score(errors: usize, warnings: usize, hints: usize) -> u8 {
    let penalty = errors * 10 + warnings * 4 + hints;
    100u8.saturating_sub(penalty.min(100) as u8)
}

/// 估算正文字数：剥掉标签与多余空白，中文按字、西文按词。
fn plain_text_len(html: &str) -> usize {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let cjk = text
        .chars()
        .filter(|c| matches!(*c, '\u{3400}'..='\u{9fff}' | '\u{3040}'..='\u{30ff}'))
        .count();
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && w.chars().any(|c| c.is_ascii_alphanumeric()))
        .count();
    cjk + words
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::SiteConfig;

    fn config() -> SiteConfig {
        let mut config = SiteConfig::default();
        config.site.base_url = "https://example.com".into();
        config.site.description = "示例站点".into();
        config
    }

    fn page(source: &str, front: &str, body: &str) -> Page {
        let raw = format!("+++\n{front}\n+++\n\n{body}\n");
        Page::from_str(
            Path::new("content"),
            Path::new(&format!("content/{source}")),
            &raw,
        )
        .unwrap()
    }

    /// 正文足够长，避免每个用例都被 content.too_short 干扰
    fn long_body() -> String {
        "内容运营需要稳定的输出节奏。".repeat(30)
    }

    /// 长度落在建议区间内的描述（≥ 40 字），避免用例被 description.too_short 干扰
    fn long_desc() -> String {
        "讲清依赖图如何定位受影响页面，以及增量构建为什么只重算这些页面，顺带说明适用范围。"
            .to_string()
    }

    fn codes(report: &Report) -> Vec<&str> {
        report.issues.iter().map(|i| i.code.as_str()).collect()
    }

    #[test]
    fn a_complete_page_has_no_issues() {
        let page = page(
            "posts/a.md",
            &format!(
                "title = \"统一模板与级联更新怎么工作\"\ndescription = \"{}\"\nkeywords = [\"模板\", \"增量构建\"]",
                long_desc()
            ),
            &long_body(),
        );

        let report = audit(&[page], &config());
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.score, 100);
        assert_eq!(report.checked, 1);
    }

    #[test]
    fn missing_description_and_keywords_are_reported() {
        let page = page("posts/a.md", "title = \"标题\"", &long_body());
        let report = audit(&[page], &config());
        assert!(codes(&report).contains(&"description.missing"));
        assert!(codes(&report).contains(&"keywords.missing"));
        assert!(report.score < 100);
    }

    #[test]
    fn keywords_fall_back_to_tags() {
        let page = page(
            "posts/a.md",
            &format!(
                "title = \"标题\"\ndescription = \"{}\"\ntags = [\"模板\"]",
                long_desc()
            ),
            &long_body(),
        );
        let report = audit(&[page], &config());
        assert!(!codes(&report).contains(&"keywords.missing"));
    }

    #[test]
    fn over_long_title_and_description_are_reported() {
        let page = page(
            "posts/a.md",
            &format!(
                "title = \"{}\"\ndescription = \"{}\"",
                "很长的标题".repeat(20),
                "很长的描述".repeat(60)
            ),
            &long_body(),
        );
        let report = audit(&[page], &config());
        let codes = codes(&report);
        assert!(codes.contains(&"title.too_long"));
        assert!(codes.contains(&"description.too_long"));
    }

    #[test]
    fn drafts_are_skipped() {
        let page = page("posts/a.md", "draft = true", "短");
        let report = audit(&[page], &config());
        assert_eq!(report.checked, 0);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn duplicate_titles_point_at_each_other() {
        let a = page(
            "posts/a.md",
            &format!(
                "title = \"同一个标题\"\ndescription = \"{}\"\ntags = [\"x\"]",
                long_desc()
            ),
            &long_body(),
        );
        let b = page(
            "posts/b.md",
            &format!(
                "title = \"同一个标题\"\ndescription = \"{}\"\ntags = [\"x\"]",
                "另一段落在建议区间内的描述，讲的是完全不同的东西，用来避开重复描述的判定。"
            ),
            &long_body(),
        );
        let report = audit(&[a, b], &config());
        let dup: Vec<&Issue> = report
            .issues
            .iter()
            .filter(|i| i.code == "title.duplicated")
            .collect();
        assert_eq!(dup.len(), 2, "两篇都该被点名：{:?}", report.issues);
        assert!(dup[0].message.contains("posts/b.md") || dup[1].message.contains("posts/b.md"));
    }

    #[test]
    fn site_level_issues_come_first() {
        let report = audit(&[], &SiteConfig::default());
        let codes = codes(&report);
        assert_eq!(codes.first(), Some(&"site.base_url_missing"));
        assert!(codes.contains(&"site.description_missing"));
        assert!(report.issues.iter().all(|i| i.source.is_empty()));
    }

    #[test]
    fn short_content_is_a_hint_only() {
        let page = page(
            "posts/a.md",
            &format!(
                "title = \"标题\"\ndescription = \"{}\"\ntags = [\"x\"]",
                long_desc()
            ),
            "只有一句话。",
        );
        let report = audit(&[page], &config());
        assert_eq!(codes(&report), vec!["content.too_short"]);
        assert_eq!(report.errors, 0);
    }

    #[test]
    fn plain_text_length_ignores_markup() {
        assert_eq!(plain_text_len("<p>你好<strong>世界</strong></p>"), 4);
        assert_eq!(plain_text_len("<p>hello brave new world</p>"), 4);
    }
}
