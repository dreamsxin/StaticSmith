//! 从别的静态站点导入内容（Hugo / Jekyll 的 YAML front matter → TOML）。
//!
//! 接手一个已有站点是最常见的入口，而卡住人的往往不是模板而是 front matter：
//! 别家用 YAML（`---` 围栏），这里用 TOML（`+++` 围栏），一篇篇手改没人愿意。
//!
//! 三条原则，都是为了「不悄悄改坏你的内容」：
//!
//! 1. **正文一个字节都不动**。只重写围栏里的元数据。
//! 2. **不认识的字段不丢**。转不了的原样写成 TOML 注释留在文件里，并在报告里
//!    列成警告——导入完你能一眼看到「哪几篇需要人看一下」。
//! 3. **先扫后导**。`scan` 只读不写，给出每篇会变成什么样；`import` 才落盘，
//!    且目标已存在时跳过而不是覆盖。
//!
//! 为什么不引 YAML 库：front matter 用到的只是 YAML 的一小块（标量、行内与块状
//! 列表），而完整 YAML 的坑（锚点、多文档、隐式类型）在这里既用不到又会带来
//! 「解析结果和别人不一样」的风险。自己认这一小块，认不出来就明说，比引一个
//! 大解析器再猜语义更可控。

use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::config::ProjectPaths;
use crate::content;
use crate::error::{Error, Result};
use crate::util;

/// 一篇待导入的内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// 相对被导入目录的路径。
    pub source: String,
    /// 会写到 `content/` 下的哪里。
    pub target: String,
    /// 转换后的 front matter（含 `+++` 围栏），预览用。
    pub front_matter: String,
    /// 需要人看一下的地方：转不了的字段、猜出来的日期等。
    pub warnings: Vec<String>,
    /// 目标已存在时为假——导入不覆盖已有内容。
    pub importable: bool,
}

/// 导入结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub imported: Vec<String>,
    pub skipped: Vec<Skipped>,
    /// 汇总的警告：`{source}: {warning}`，方便直接打印。
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub source: String,
    pub reason: String,
}

/// 扫描一个目录，给出每篇会变成什么样。只读。
///
/// `section` 是导入到哪个栏目；目录层级会被保留，`posts/2026/a.md` 进
/// `content/{section}/2026/a.md`。
pub fn scan(paths: &ProjectPaths, from: &Path, section: &str) -> Result<Vec<Candidate>> {
    if !from.is_dir() {
        return Err(Error::Other(format!("{} 不是目录", from.display())));
    }
    let section = util::sanitize_relative_dir(section);
    let mut out = Vec::new();

    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() || !is_markdown(entry.path()) {
            continue;
        }
        let relative = util::to_slash(entry.path().strip_prefix(from).unwrap_or(entry.path()));
        let raw = std::fs::read_to_string(entry.path()).map_err(|e| Error::io(entry.path(), e))?;
        let converted = convert(&raw, &relative);
        let target = target_path(&section, &relative, &converted.slug_from_name);
        let full = content::resolve_source(&paths.content, &target)?;
        out.push(Candidate {
            source: relative,
            target,
            front_matter: converted.front_matter,
            warnings: converted.warnings,
            importable: !full.exists(),
        });
    }
    out.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(out)
}

/// 导入。目标已存在的跳过，不覆盖。
pub fn import(paths: &ProjectPaths, from: &Path, section: &str) -> Result<Report> {
    let candidates = scan(paths, from, section)?;
    let mut report = Report::default();

    for candidate in candidates {
        if !candidate.importable {
            report.skipped.push(Skipped {
                source: candidate.source,
                reason: "目标已存在，未覆盖".to_string(),
            });
            continue;
        }
        let origin = from.join(&candidate.source);
        let raw = std::fs::read_to_string(&origin).map_err(|e| Error::io(&origin, e))?;
        let body = body_of(&raw);
        let target = content::resolve_source(&paths.content, &candidate.target)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let text = format!("{}\n{}", candidate.front_matter, body);
        std::fs::write(&target, text).map_err(|e| Error::io(&target, e))?;

        for warning in &candidate.warnings {
            report
                .warnings
                .push(format!("{}: {warning}", candidate.source));
        }
        report.imported.push(candidate.target);
    }
    Ok(report)
}

/// 转换结果。
struct Converted {
    front_matter: String,
    warnings: Vec<String>,
    /// Jekyll 的 `2026-01-02-标题.md` 去掉日期前缀后的文件名。
    slug_from_name: Option<String>,
}

/// 目标路径：保留目录层级，去掉 Jekyll 的日期前缀。
fn target_path(section: &str, relative: &str, renamed: &Option<String>) -> String {
    let mut parts: Vec<String> = relative.split('/').map(|s| s.to_string()).collect();
    if let (Some(last), Some(new_name)) = (parts.last_mut(), renamed) {
        *last = new_name.clone();
    }
    let joined = parts.join("/");
    if section.is_empty() {
        joined
    } else {
        format!("{section}/{joined}")
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

/// 正文（去掉 front matter 围栏）。认 `---` 与 `+++` 两种。
fn body_of(raw: &str) -> String {
    match split(raw) {
        Some((_, _, body)) => body.to_string(),
        None => raw.to_string(),
    }
}

/// 拆出围栏类型、围栏内文本、正文。
fn split(raw: &str) -> Option<(Fence, &str, &str)> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    for (fence, marker) in [(Fence::Yaml, "---"), (Fence::Toml, "+++")] {
        let Some(rest) = text.strip_prefix(marker) else {
            continue;
        };
        // 围栏标记必须独占一行
        let rest = rest.strip_prefix('\r').unwrap_or(rest);
        let Some(rest) = rest.strip_prefix('\n') else {
            continue;
        };
        let end = format!("\n{marker}");
        if let Some(at) = rest.find(&end) {
            let inner = &rest[..at];
            let after = &rest[at + end.len()..];
            let body = after
                .strip_prefix('\r')
                .unwrap_or(after)
                .strip_prefix('\n')
                .unwrap_or(after);
            return Some((fence, inner, body));
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Yaml,
    Toml,
}

/// 把一篇的 front matter 转成 TOML 围栏。
fn convert(raw: &str, relative: &str) -> Converted {
    let mut warnings = Vec::new();
    let (fence, inner) = match split(raw) {
        Some((fence, inner, _)) => (fence, inner.to_string()),
        None => {
            warnings.push("没有 front matter，已按标题=文件名补一个".to_string());
            (Fence::Yaml, String::new())
        }
    };

    // 已经是 TOML 的直接留用：从另一个 StaticSmith / Zola 站点搬内容时不该被重写
    if fence == Fence::Toml {
        return Converted {
            front_matter: format!("+++\n{}\n+++", inner.trim_end()),
            warnings,
            slug_from_name: None,
        };
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let parsed = parse_yaml_subset(&inner, &mut warnings);
    let mut has_title = false;
    let mut has_date = false;

    for (key, value) in parsed {
        match (key.as_str(), value) {
            (_, YamlValue::Unsupported(text)) => {
                warnings.push(format!("字段 {key} 没转换，已作为注释保留"));
                comments.push(format!("# {key}: {text}"));
            }
            // Jekyll 用 published 表达「发不发」，与这里的 draft 正好相反
            ("published", YamlValue::Bool(published)) => {
                fields.push(("draft".to_string(), (!published).to_string()));
            }
            // layout 是别家的模板名，猜错会让整页渲染错，只提示不映射
            ("layout", value) => {
                warnings.push(format!(
                    "layout = {} 未映射，需要时自己写 template",
                    value.render()
                ));
                comments.push(format!("# layout: {}", value.render_plain()));
            }
            (key, value) => {
                if key == "title" {
                    has_title = true;
                }
                if key == "date" {
                    has_date = true;
                    // Jekyll/Hugo 常见的 `2026-02-03 10:20:00 +0800` 既不是 RFC3339
                    // 也不是纯日期，照抄进去会被解析成「没有日期」——静默丢掉最难查
                    let (normalized, warning) = normalize_date(&value.render_plain());
                    if let Some(warning) = warning {
                        warnings.push(warning);
                    }
                    fields.push((key.to_string(), render_string(&normalized)));
                    continue;
                }
                fields.push((key.to_string(), value.render()));
            }
        }
    }

    let file_stem = relative
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(stem, _)| stem.to_string())
        .unwrap_or_else(|| relative.to_string());

    // Jekyll 的 _posts/2026-01-02-标题.md：日期在文件名里
    let dated = file_stem
        .split_once('-')
        .and_then(|(y, rest)| rest.split_once('-').map(|(m, d)| (y, m, d)))
        .and_then(|(y, m, rest)| {
            rest.split_once('-').map(|(d, name)| {
                (
                    y.to_string(),
                    m.to_string(),
                    d.to_string(),
                    name.to_string(),
                )
            })
        })
        .filter(|(y, m, d, name)| {
            y.len() == 4
                && y.chars().all(|c| c.is_ascii_digit())
                && m.len() == 2
                && m.chars().all(|c| c.is_ascii_digit())
                && d.len() == 2
                && d.chars().all(|c| c.is_ascii_digit())
                && !name.is_empty()
        });

    let mut slug_from_name = None;
    if let Some((y, m, d, name)) = dated {
        if !has_date {
            fields.insert(0, ("date".to_string(), format!("\"{y}-{m}-{d}\"")));
            has_date = true;
            warnings.push(format!("日期取自文件名：{y}-{m}-{d}"));
        }
        slug_from_name = Some(format!("{name}.md"));
    }

    if !has_title {
        let fallback = slug_from_name
            .as_deref()
            .and_then(|n| n.strip_suffix(".md"))
            .unwrap_or(&file_stem)
            .to_string();
        fields.insert(0, ("title".to_string(), render_string(&fallback)));
        warnings.push("没有 title，已用文件名补上".to_string());
    }
    let _ = has_date;

    let mut lines = vec!["+++".to_string()];
    for (key, value) in &fields {
        lines.push(format!("{key} = {value}"));
    }
    lines.extend(comments);
    lines.push("+++".to_string());

    Converted {
        front_matter: lines.join("\n"),
        warnings,
        slug_from_name,
    }
}

/// YAML 子集里的值。
enum YamlValue {
    Str(String),
    Bare(String),
    Bool(bool),
    List(Vec<String>),
    /// 认不出来的原样文本，交给调用方写成注释。
    Unsupported(String),
}

impl YamlValue {
    /// 渲染成 TOML 值。
    fn render(&self) -> String {
        match self {
            YamlValue::Str(text) => render_string(text),
            YamlValue::Bare(text) => text.clone(),
            YamlValue::Bool(value) => value.to_string(),
            YamlValue::List(items) => {
                let rendered: Vec<String> = items.iter().map(|i| render_string(i)).collect();
                format!("[{}]", rendered.join(", "))
            }
            YamlValue::Unsupported(text) => render_string(text),
        }
    }

    /// 注释里用的原样文本。
    fn render_plain(&self) -> String {
        match self {
            YamlValue::Str(text) | YamlValue::Bare(text) | YamlValue::Unsupported(text) => {
                text.clone()
            }
            YamlValue::Bool(value) => value.to_string(),
            YamlValue::List(items) => items.join(", "),
        }
    }
}

fn render_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 把别家的日期写法归一成这里认的两种：纯日期或 RFC3339。
///
/// 构建只认 `YYYY-MM-DD` 与 RFC3339，而 Jekyll/Hugo 里常见的
/// `2026-02-03 10:20:00 +0800` 两种都不是——照抄进去会被当成「没有日期」，
/// 文章排序与订阅时间全错，而且没有任何报错，最难查。
///
/// 认不出来的原样留下并给出警告：宁可让人看一眼，也不猜。
fn normalize_date(raw: &str) -> (String, Option<String>) {
    use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

    let text = raw.trim();
    if text.is_empty() {
        return (text.to_string(), None);
    }
    // 已经合规的两种写法直接留用
    if NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
        || DateTime::parse_from_rfc3339(text).is_ok()
    {
        return (text.to_string(), None);
    }

    // 带时区的：+0800 / +08:00 / Z，日期与时间之间可能是空格
    for format in [
        "%Y-%m-%d %H:%M:%S %z",
        "%Y-%m-%d %H:%M:%S%z",
        "%Y-%m-%d %H:%M %z",
        "%Y-%m-%d %H:%M%z",
        "%Y-%m-%dT%H:%M:%S%z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(text, format) {
            return (dt.to_rfc3339(), None);
        }
    }

    // 不带时区的：按 UTC 处理，并说清这个假设
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            let dt: DateTime<Utc> = naive.and_utc();
            return (
                dt.to_rfc3339(),
                Some(format!("日期 {text} 没写时区，按 UTC 处理")),
            );
        }
    }

    (
        text.to_string(),
        Some(format!("日期 {text} 认不出来，构建时会被当成没有日期")),
    )
}

/// 解析 front matter 用到的那一小块 YAML。
///
/// 认：`key: 值`、`key: [a, b]`、`key:` 之后的 `- 列表项`。
/// 不认的（嵌套映射、多行文本块等）整段回传成 `Unsupported`，由调用方写成注释——
/// 静默丢字段比报错更糟。
fn parse_yaml_subset(inner: &str, warnings: &mut Vec<String>) -> Vec<(String, YamlValue)> {
    let lines: Vec<&str> = inner.lines().collect();
    let mut out: Vec<(String, YamlValue)> = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // 顶层字段一定不缩进；缩进行要么已被列表分支吃掉，要么就是不认识的结构
        if line.starts_with(' ') || line.starts_with('\t') {
            warnings.push(format!("跳过看不懂的一行：{trimmed}"));
            continue;
        }
        let Some((key, rest)) = trimmed.split_once(':') else {
            warnings.push(format!("跳过看不懂的一行：{trimmed}"));
            continue;
        };
        let key = key.trim().to_string();
        let rest = rest.trim();

        if rest.is_empty() {
            // 可能跟着块状列表
            let mut items = Vec::new();
            while index < lines.len() {
                let next = lines[index];
                let next_trimmed = next.trim();
                if next_trimmed.starts_with("- ") || next_trimmed == "-" {
                    items.push(unquote(next_trimmed.trim_start_matches('-').trim()));
                    index += 1;
                    continue;
                }
                if next.starts_with(' ') || next.starts_with('\t') {
                    // 缩进但不是列表：嵌套映射之类，原样交出去
                    let mut block = Vec::new();
                    while index < lines.len()
                        && (lines[index].starts_with(' ') || lines[index].starts_with('\t'))
                    {
                        block.push(lines[index].trim().to_string());
                        index += 1;
                    }
                    out.push((key.clone(), YamlValue::Unsupported(block.join(" / "))));
                    break;
                }
                break;
            }
            if !items.is_empty() {
                out.push((key, YamlValue::List(items)));
            } else if !matches!(out.last(), Some((last, _)) if *last == key) {
                out.push((key, YamlValue::Str(String::new())));
            }
            continue;
        }

        out.push((key, scalar(rest)));
    }
    out
}

/// 行内标量与行内列表。
fn scalar(text: &str) -> YamlValue {
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let items: Vec<String> = inner
            .split(',')
            .map(|item| unquote(item.trim()))
            .filter(|item| !item.is_empty())
            .collect();
        return YamlValue::List(items);
    }
    match text {
        "true" | "yes" => return YamlValue::Bool(true),
        "false" | "no" => return YamlValue::Bool(false),
        _ => {}
    }
    // 纯数字直接当 TOML 数字，别加引号（weight 这类字段要的是整数）
    if text.parse::<i64>().is_ok() || text.parse::<f64>().is_ok() {
        return YamlValue::Bare(text.to_string());
    }
    YamlValue::Str(unquote(text))
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            return text[1..text.len() - 1].to_string();
        }
    }
    text.to_string()
}

/// 便于上层给出「导入到哪」的提示。
pub fn default_section() -> PathBuf {
    PathBuf::from("posts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Assets, Build};

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: ProjectPaths,
        from: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), &Build::default(), &Assets::default());
        std::fs::create_dir_all(&paths.content).unwrap();
        let from = dir.path().join("incoming");
        std::fs::create_dir_all(&from).unwrap();
        Fixture {
            _dir: dir,
            paths,
            from,
        }
    }

    fn write(f: &Fixture, relative: &str, raw: &str) {
        let path = relative
            .split('/')
            .fold(f.from.clone(), |acc, part| acc.join(part));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    #[test]
    fn dates_are_normalized_to_what_the_builder_accepts() {
        // 纯日期与 RFC3339 原样留用
        assert_eq!(normalize_date("2026-01-02").0, "2026-01-02");
        assert_eq!(
            normalize_date("2026-01-02T03:04:05+08:00").0,
            "2026-01-02T03:04:05+08:00"
        );

        // Jekyll 常见写法：空格分隔 + 紧凑时区，转成 RFC3339
        let (value, warning) = normalize_date("2026-02-03 10:20:00 +0800");
        assert_eq!(value, "2026-02-03T10:20:00+08:00");
        assert!(warning.is_none());

        // 没写时区就按 UTC，并说清这个假设
        let (value, warning) = normalize_date("2026-02-03 10:20:00");
        assert_eq!(value, "2026-02-03T10:20:00+00:00");
        assert!(warning.unwrap().contains("按 UTC"));

        // 认不出来的原样留下，但要提醒否则构建时会静默没有日期
        let (value, warning) = normalize_date("去年春天");
        assert_eq!(value, "去年春天");
        assert!(warning.unwrap().contains("认不出来"));
    }

    #[test]
    fn imported_dates_survive_the_content_parser() {
        let f = fixture();
        write(
            &f,
            "dated.md",
            "---\ntitle: 有时间的\ndate: 2026-02-03 10:20:00 +0800\n---\n正文\n",
        );
        import(&f.paths, &f.from, "posts").unwrap();

        // 关键：导入后的文件被内容解析器读出来必须真的有日期
        let pages = content::load_all(&f.paths.content, Default::default()).unwrap();
        let page = pages.iter().find(|p| p.source == "posts/dated.md").unwrap();
        assert!(page.date.is_some(), "日期不能在导入后丢掉");
    }

    #[test]
    fn hugo_style_yaml_becomes_toml_and_keeps_the_body() {
        let f = fixture();
        write(
            &f,
            "hello.md",
            "---\ntitle: \"你好，世界\"\ndate: 2026-01-02\ndraft: true\nweight: 3\ntags: [模板, 运营]\ncategories:\n  - 随笔\n  - 技术\n---\n\n正文第一段。\n",
        );

        let candidates = scan(&f.paths, &f.from, "posts").unwrap();
        assert_eq!(candidates.len(), 1);
        let fm = &candidates[0].front_matter;
        assert!(fm.starts_with("+++"), "{fm}");
        assert!(fm.contains("title = \"你好，世界\""), "{fm}");
        assert!(fm.contains("date = \"2026-01-02\""), "{fm}");
        assert!(fm.contains("draft = true"), "{fm}");
        assert!(fm.contains("weight = 3"), "数字不加引号：{fm}");
        assert!(fm.contains("tags = [\"模板\", \"运营\"]"), "{fm}");
        assert!(
            fm.contains("categories = [\"随笔\", \"技术\"]"),
            "块状列表：{fm}"
        );
        assert_eq!(candidates[0].target, "posts/hello.md");

        let report = import(&f.paths, &f.from, "posts").unwrap();
        assert_eq!(report.imported, vec!["posts/hello.md".to_string()]);
        let written = std::fs::read_to_string(
            content::resolve_source(&f.paths.content, "posts/hello.md").unwrap(),
        )
        .unwrap();
        assert!(written.contains("正文第一段。"), "正文必须原样：{written}");
        assert!(written.starts_with("+++\n"));
    }

    #[test]
    fn jekyll_conventions_are_translated() {
        let f = fixture();
        // 日期在文件名里、published 与 draft 相反、layout 不猜
        write(
            &f,
            "_posts/2026-03-05-my-post.md",
            "---\ntitle: 旧站文章\npublished: false\nlayout: post\n---\n正文\n",
        );

        let candidates = scan(&f.paths, &f.from, "posts").unwrap();
        let candidate = &candidates[0];
        assert_eq!(
            candidate.target, "posts/_posts/my-post.md",
            "去掉日期前缀，保留目录层级"
        );
        assert!(candidate.front_matter.contains("date = \"2026-03-05\""));
        assert!(
            candidate.front_matter.contains("draft = true"),
            "published: false → draft"
        );
        assert!(
            candidate.front_matter.contains("# layout: post"),
            "layout 不猜但要留痕：{}",
            candidate.front_matter
        );
        assert!(candidate.warnings.iter().any(|w| w.contains("layout")));
        assert!(candidate
            .warnings
            .iter()
            .any(|w| w.contains("日期取自文件名")));
    }

    #[test]
    fn unsupported_fields_are_kept_as_comments_not_dropped() {
        let f = fixture();
        write(
            &f,
            "nested.md",
            "---\ntitle: 有嵌套\nauthor:\n  name: 张三\n  email: a@b.c\n---\n正文\n",
        );

        let candidates = scan(&f.paths, &f.from, "").unwrap();
        let candidate = &candidates[0];
        assert!(
            candidate
                .front_matter
                .contains("# author: name: 张三 / email: a@b.c"),
            "{}",
            candidate.front_matter
        );
        assert!(candidate.warnings.iter().any(|w| w.contains("author")));
        assert_eq!(candidate.target, "nested.md", "栏目留空就进根目录");
    }

    #[test]
    fn toml_front_matter_is_left_alone() {
        let f = fixture();
        write(
            &f,
            "already.md",
            "+++\ntitle = \"本来就是 TOML\"\ntags = [\"甲\"]\n+++\n正文\n",
        );

        let candidates = scan(&f.paths, &f.from, "posts").unwrap();
        assert_eq!(
            candidates[0].front_matter,
            "+++\ntitle = \"本来就是 TOML\"\ntags = [\"甲\"]\n+++"
        );
        assert!(candidates[0].warnings.is_empty());
    }

    #[test]
    fn missing_front_matter_gets_a_title_from_the_file_name() {
        let f = fixture();
        write(&f, "notes/纯正文.md", "只有正文，没有围栏。\n");

        let candidates = scan(&f.paths, &f.from, "posts").unwrap();
        assert!(candidates[0].front_matter.contains("title = \"纯正文\""));
        assert!(candidates[0]
            .warnings
            .iter()
            .any(|w| w.contains("没有 front matter")));

        import(&f.paths, &f.from, "posts").unwrap();
        let written = std::fs::read_to_string(
            content::resolve_source(&f.paths.content, "posts/notes/纯正文.md").unwrap(),
        )
        .unwrap();
        assert!(written.contains("只有正文，没有围栏。"));
    }

    #[test]
    fn existing_targets_are_never_overwritten() {
        let f = fixture();
        write(&f, "hello.md", "---\ntitle: 新的\n---\n新正文\n");
        let target = content::resolve_source(&f.paths.content, "posts/hello.md").unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "+++\ntitle = \"原有的\"\n+++\n原有正文\n").unwrap();

        let candidates = scan(&f.paths, &f.from, "posts").unwrap();
        assert!(!candidates[0].importable);

        let report = import(&f.paths, &f.from, "posts").unwrap();
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].reason.contains("未覆盖"));
        assert!(std::fs::read_to_string(&target)
            .unwrap()
            .contains("原有正文"));
    }

    #[test]
    fn non_markdown_and_missing_dir_are_handled() {
        let f = fixture();
        write(&f, "a.txt", "不是 markdown");
        assert!(scan(&f.paths, &f.from, "posts").unwrap().is_empty());
        assert!(scan(&f.paths, &f.from.join("nope"), "posts").is_err());
    }
}
