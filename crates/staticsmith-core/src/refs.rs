//! 搬动之后改写站内引用（Dreamweaver 的 Update Links 那件事）。
//!
//! 搬走一篇文章会改它的地址。补 `aliases` 只是让**外部**旧链接经重定向页继续可用，
//! 站内其它文章里那些 `[见增量构建](/posts/incremental/)` 仍然指着旧地址——
//! 多跳一次重定向是轻的，真正的问题是这些引用从此再也不会跟着源头动，
//! 攒上几次搬动之后就没人说得清哪条还对。专业的做法是搬完顺手把引用改过来。
//!
//! 三条自我约束：
//!
//! 1. **只改正文**。front matter 一律不碰：那里的 `aliases` 记的正是旧地址，
//!    改掉它等于把刚建好的重定向拆了。
//! 2. **匹配已知的地址，不解析 Markdown**。要改的是「站点里确实存在的那个地址」，
//!    所以拿旧地址当针去正文里找，前后必须是分隔符（`(`、`"`、`'`、`<`、空格、`:`）
//!    与终止符（`)`、`>`、引号、`#`、`?`、空白、行尾）。这条规则同时覆盖
//!    Markdown 链接、图片、`<a href>` / `src`、`[id]:` 引用式定义与 `<地址>` 形式，
//!    而不必为每种语法写一套解析器——`replace` 模块也是这个路子（纯文本、不做正则）。
//! 3. **围栏代码块里的不动**。文档里贴出来的地址是教学材料，不是引用。
//!
//! 已知不管的两种：行内代码 `` `/posts/a/` `` 里的地址会被改（与围栏不同，
//! 行内代码常常就是在说这个链接），以及相对地址（`../a/`）——它要按引用方所在目录
//! 解析，而搬动改变的是被引用方，两者对不上，留给「死链体检」发现。

use std::path::Path;

use serde::Serialize;

use crate::config::SourceFormat;
use crate::content;
use crate::error::{Error, Result};
use crate::frontmatter;

/// 一次地址变更：从旧地址到新地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UrlMove {
    pub from: String,
    pub to: String,
}

/// 某一篇里被改写（或将被改写）的引用条数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefUpdate {
    pub source: String,
    pub hits: usize,
}

/// 地址前一个字符必须是这些之一：链接语法里地址总跟在分隔符后面。
///
/// 不允许「行首」：行首的裸地址多半是正文里在讲这个地址，不是引用。
const OPENERS: &[u8] = b"(\"'< \t:";

/// 地址后一个字符必须是这些之一，或到行尾。
///
/// 少了这一条会误伤：`/posts/a` 是 `/posts/ab` 的前缀，`/posts/a/` 是 `/posts/a/b/` 的前缀。
const CLOSERS: &[u8] = b")>\"'#? \t\r\n,";

/// 改写一份源文里的站内引用。
///
/// 返回 `None` 表示没有命中，调用方据此跳过写盘。`Some((新源文, 命中数))`。
/// front matter 读不出来（缺结束围栏）时报错而不是猜——猜错会把正文当 front matter 改。
pub fn rewrite(raw: &str, moves: &[UrlMove]) -> Result<Option<(String, usize)>> {
    if moves.is_empty() {
        return Ok(None);
    }
    let start = frontmatter::body_start(raw)?;
    let (head, body) = raw.split_at(start);

    let mut out = String::with_capacity(raw.len());
    let mut hits = 0usize;
    let mut fence: Option<String> = None;

    for line in body.split_inclusive('\n') {
        match &fence {
            // 围栏内原样抄，只看它是不是收尾行
            Some(open) => {
                out.push_str(line);
                if closes_fence(line, open) {
                    fence = None;
                }
                continue;
            }
            None => {
                if let Some(open) = opens_fence(line) {
                    fence = Some(open);
                    out.push_str(line);
                    continue;
                }
            }
        }
        hits += rewrite_line(line, moves, &mut out);
    }

    if hits == 0 {
        return Ok(None);
    }
    let mut result = String::with_capacity(head.len() + out.len());
    result.push_str(head);
    result.push_str(&out);
    Ok(Some((result, hits)))
}

/// 全站改写的结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Rewritten {
    /// 改了（或将要改）哪几篇、各几处。
    pub updated: Vec<RefUpdate>,
    /// 没能改的那几篇及原因。多半是 front matter 手改坏了。
    pub failed: Vec<Failure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub source: String,
    pub reason: String,
}

/// 干跑：全站有多少处引用会被改。不碰磁盘。
pub fn preview_site(content_root: &Path, moves: &[UrlMove]) -> Result<Rewritten> {
    run(content_root, moves, false)
}

/// 全站改写并写回。
///
/// 搬动、改 slug、栏目改名三条路都用它：地址变了就该把指向它的链接改过来，
/// 三处各写一遍循环，迟早有一处漏掉「写盘失败要报出来」这类细节。
pub fn rewrite_site(content_root: &Path, moves: &[UrlMove]) -> Result<Rewritten> {
    run(content_root, moves, true)
}

/// 干跑与执行共用一份遍历，差别只在最后写不写盘（同 `replace` 模块）。
///
/// 这里固定用默认正文格式加载：改写只看源文本身，与站点的 `source_format` 无关，
/// 而 `load_all` 收哪些扩展名也与格式无关——HTML 站点的 `.html` 同样在列。
fn run(content_root: &Path, moves: &[UrlMove], write: bool) -> Result<Rewritten> {
    let mut out = Rewritten::default();
    if moves.is_empty() {
        return Ok(out);
    }
    // 只列文件、不解析 front matter：改写只看源文本身，用不着解析结果，
    // 而 `load_all` 会因为一篇坏文件让整次改写（连干跑）都做不了
    for source in content::source_files(content_root)? {
        let path = match content::resolve_source(content_root, &source) {
            Ok(path) => path,
            Err(err) => {
                out.failed.push(Failure {
                    source,
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| Error::io(&path, e))
            .and_then(|raw| rewrite(&raw, moves));
        match result {
            Ok(None) => {}
            Ok(Some((updated, hits))) => {
                if write {
                    if let Err(err) =
                        std::fs::write(&path, updated).map_err(|e| Error::io(&path, e))
                    {
                        out.failed.push(Failure {
                            source,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                }
                out.updated.push(RefUpdate { source, hits });
            }
            Err(err) => out.failed.push(Failure {
                source,
                reason: err.to_string(),
            }),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- 相对链接

/// 正文里、围栏之外的那些行。
fn body_lines(raw: &str) -> Result<Vec<&str>> {
    let start = frontmatter::body_start(raw)?;
    let mut out = Vec::new();
    let mut fence: Option<String> = None;
    for line in raw[start..].split_inclusive('\n') {
        match &fence {
            Some(open) => {
                if closes_fence(line, open) {
                    fence = None;
                }
            }
            None => match opens_fence(line) {
                Some(open) => fence = Some(open),
                None => out.push(line),
            },
        }
    }
    Ok(out)
}

/// 一行里出现的链接地址：Markdown 的 `](…)` 与 HTML 的 `href=` / `src=`。
///
/// 与改写那条路不同，这里必须真的把地址取出来（改写只需要拿已知地址去比对），
/// 所以认标记而不是认分隔符。取到的是原样文本，是否相对由调用方判断。
fn destinations(line: &str) -> Vec<&str> {
    const MARKS: [(&str, char); 5] = [
        ("](", ')'),
        ("href=\"", '"'),
        ("href='", '\''),
        ("src=\"", '"'),
        ("src='", '\''),
    ];
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let Some((at, mark, end)) = MARKS
            .iter()
            .filter_map(|(mark, end)| rest.find(mark).map(|at| (at, *mark, *end)))
            .min_by_key(|(at, _, _)| *at)
        else {
            break;
        };
        let after = &rest[at + mark.len()..];
        let value = match after.find(end) {
            Some(stop) => &after[..stop],
            None => after,
        };
        // Markdown 允许 `](<地址> "标题")`：尖括号与标题都不是地址的一部分
        let value = value.trim();
        let value = value.trim_start_matches('<').trim_end_matches('>');
        let value = value.split_whitespace().next().unwrap_or("");
        if !value.is_empty() {
            out.push(value);
        }
        rest = &after[value.len().min(after.len())..];
    }
    out
}

/// 这段地址是不是「相对」的——也就是要按引用方所在目录才能解析。
fn is_relative(target: &str) -> bool {
    if target.starts_with('/') || target.starts_with('#') || target.starts_with('?') {
        return false;
    }
    // `https://`、`mailto:`、`data:`：第一个 `:` 出现在第一个 `/` 之前就是协议
    match (target.find(':'), target.find('/')) {
        (Some(colon), Some(slash)) => colon > slash,
        (Some(_), None) => false,
        _ => true,
    }
}

/// 一段相对地址在引用方页面上解析成什么绝对地址。
///
/// 页面地址本身就是目录（pretty URL，恒以 `/` 结尾），所以 `../a/` 是「同栏目的 a」。
/// 走出站点根返回 `None`：那不是站内链接，别猜。
fn resolve_relative(base: &str, target: &str) -> Option<String> {
    let trailing = target.ends_with('/');
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    Some(if trailing {
        format!("/{joined}/")
    } else {
        format!("/{joined}")
    })
}

/// 这一篇里有几处**相对**链接指向即将改变的地址。
///
/// 这些是改写改不到的：相对地址要按引用方所在目录解析，而改地址改的是被引用方，
/// 两者对不上。以前它们只会在「死链体检」里出现——那是**改完之后**。
/// 干跑里报出来，用户才有机会在动手前决定怎么处理。
pub fn relative_hits(raw: &str, base_url: &str, moves: &[UrlMove]) -> Result<usize> {
    if moves.is_empty() {
        return Ok(0);
    }
    let mut hits = 0;
    for line in body_lines(raw)? {
        for target in destinations(line) {
            if !is_relative(target) {
                continue;
            }
            let Some(resolved) = resolve_relative(base_url, target) else {
                continue;
            };
            let normalized = resolved.trim_end_matches('/');
            if moves
                .iter()
                .any(|item| item.from.trim_end_matches('/') == normalized)
            {
                hits += 1;
            }
        }
    }
    Ok(hits)
}

/// 全站扫一遍：哪几篇里有相对链接指向即将改变的地址，各几处。
///
/// 与 `preview_site` 并列使用：那份是「会自动改好的」，这份是「得你自己看一眼的」。
pub fn manual_review(content_root: &Path, moves: &[UrlMove]) -> Result<Vec<RefUpdate>> {
    if moves.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    // 逐篇解析而不是 `load_all`：这一步只是提醒，不该因为站里有一篇坏文件就整体失败
    // （那时用户正要靠这份提醒决定动不动手）。坏的那篇跳过，真正的写操作会单独报它
    for source in content::source_files(content_root)? {
        let Ok(path) = content::resolve_source(content_root, &source) else {
            continue;
        };
        let Ok(page) = content::Page::from_file(content_root, &path, SourceFormat::default())
        else {
            continue;
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(hits) = relative_hits(&raw, &page.url, moves) {
            if hits > 0 {
                out.push(RefUpdate { source, hits });
            }
        }
    }
    Ok(out)
}

/// 一行里的改写，命中数作为返回值。
fn rewrite_line(line: &str, moves: &[UrlMove], out: &mut String) -> usize {
    let bytes = line.as_bytes();
    let mut hits = 0usize;
    let mut copied = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'/' || i == 0 || !OPENERS.contains(&bytes[i - 1]) {
            i += 1;
            continue;
        }
        match match_at(&line[i..], moves) {
            None => i += 1,
            Some((len, replacement)) => {
                out.push_str(&line[copied..i]);
                out.push_str(&replacement);
                hits += 1;
                i += len;
                copied = i;
            }
        }
    }
    out.push_str(&line[copied..]);
    hits
}

/// `rest` 开头是不是某个旧地址？命中则给出「吃掉多少字节」与替换文本。
///
/// 带尾斜杠的写法先试：`/a/b` 是 `/a/b/` 的前缀，反过来试会把后者截成前者。
/// 作者写的是哪种风格就还给他哪种——把 `/a/b` 改成 `/a/b/` 属于顺手改人家的正文。
fn match_at(rest: &str, moves: &[UrlMove]) -> Option<(usize, String)> {
    for item in moves {
        let slashed = item.from.trim_end_matches('/');
        for (needle, replacement) in [
            (format!("{slashed}/"), item.to.clone()),
            (
                slashed.to_string(),
                item.to.trim_end_matches('/').to_string(),
            ),
        ] {
            if needle.is_empty() || !rest.starts_with(&needle) {
                continue;
            }
            let after = rest.as_bytes().get(needle.len());
            if after.is_none_or(|byte| CLOSERS.contains(byte)) {
                return Some((needle.len(), replacement));
            }
        }
    }
    None
}

/// 这一行是不是围栏起始？返回围栏本身（``` 或 ~~~，含长度）。
///
/// 长度要记住：CommonMark 允许更长的围栏包住内含三个反引号的代码，
/// 收尾的围栏不能比开头短。
fn opens_fence(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    for marker in ['`', '~'] {
        let run = trimmed.chars().take_while(|c| *c == marker).count();
        if run >= 3 {
            return Some(marker.to_string().repeat(run));
        }
    }
    None
}

fn closes_fence(line: &str, open: &str) -> bool {
    let trimmed = line.trim();
    let marker = open.as_bytes()[0] as char;
    trimmed.len() >= open.len()
        && trimmed.chars().all(|c| c == marker)
        && trimmed.chars().take_while(|c| *c == marker).count() >= open.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moves(from: &str, to: &str) -> Vec<UrlMove> {
        vec![UrlMove {
            from: from.to_string(),
            to: to.to_string(),
        }]
    }

    fn rewritten(raw: &str, moves: &[UrlMove]) -> (String, usize) {
        rewrite(raw, moves).unwrap().expect("应当有命中")
    }

    /// 一篇坏文件不该让整次链接改写失败。
    ///
    /// 搬动、改 slug、栏目改名三条路都要先算「哪几篇里的链接会改」。以前这一步走
    /// `load_all`，站里有一篇 front matter 手改坏了，整个搬动连干跑都做不了——
    /// 而这时用户最需要的恰恰是「其余的照改，这一篇单独报出来」。
    #[test]
    fn a_broken_file_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("posts")).unwrap();
        std::fs::write(
            root.join("posts/ok.md"),
            "+++\ntitle = \"好的\"\n+++\n\n见 [甲](/posts/a/)。\n",
        )
        .unwrap();
        // 缺结束围栏
        std::fs::write(
            root.join("posts/broken.md"),
            "+++\ntitle = \"坏的\"\n\n见 [甲](/posts/a/)。\n",
        )
        .unwrap();

        let out = rewrite_site(root, &moves("/posts/a/", "/notes/a/")).unwrap();
        assert_eq!(out.updated.len(), 1, "{out:?}");
        assert_eq!(out.updated[0].source, "posts/ok.md");
        assert_eq!(out.failed.len(), 1, "{out:?}");
        assert_eq!(out.failed[0].source, "posts/broken.md");
        assert!(out.failed[0].reason.contains("+++"), "{out:?}");
        // 坏的那篇一个字节都没动
        let raw = std::fs::read_to_string(root.join("posts/broken.md")).unwrap();
        assert!(raw.contains("/posts/a/"), "{raw}");
    }

    #[test]
    fn markdown_link_is_rewritten() {
        let raw = "+++\ntitle = \"甲\"\n+++\n\n见 [增量构建](/posts/incremental/)。\n";
        let (out, hits) = rewritten(raw, &moves("/posts/incremental/", "/notes/incremental/"));
        assert_eq!(hits, 1);
        assert!(out.contains("[增量构建](/notes/incremental/)"));
    }

    #[test]
    fn author_style_of_trailing_slash_is_kept() {
        let raw = "+++\n+++\n\n[甲](/posts/a) 与 [乙](/posts/a/)\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 2);
        assert!(out.contains("[甲](/notes/a)"), "{out}");
        assert!(out.contains("[乙](/notes/a/)"), "{out}");
    }

    #[test]
    fn anchor_and_query_survive() {
        let raw = "+++\n+++\n\n[甲](/posts/a/#小节) [乙](/posts/a/?from=rss)\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 2);
        assert!(out.contains("(/notes/a/#小节)"), "{out}");
        assert!(out.contains("(/notes/a/?from=rss)"), "{out}");
    }

    #[test]
    fn longer_paths_are_not_touched() {
        // /posts/a 是 /posts/ab 的前缀，/posts/a/ 是 /posts/a/b/ 的前缀
        let raw = "+++\n+++\n\n[甲](/posts/ab/) [乙](/posts/a/b/) [丙](/posts/abc)\n";
        assert!(rewrite(raw, &moves("/posts/a/", "/notes/a/"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn html_attributes_are_rewritten() {
        let raw = "+++\n+++\n\n<a href=\"/posts/a/\">甲</a> <img src='/posts/a' />\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 2);
        assert!(out.contains("href=\"/notes/a/\""), "{out}");
        assert!(out.contains("src='/notes/a'"), "{out}");
    }

    #[test]
    fn angle_form_and_reference_definition_are_rewritten() {
        let raw = "+++\n+++\n\n见 [甲](</posts/a/>)\n\n[甲]: /posts/a/ \"标题\"\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 2);
        assert!(out.contains("(</notes/a/>)"), "{out}");
        assert!(out.contains("[甲]: /notes/a/ \"标题\""), "{out}");
    }

    #[test]
    fn fenced_code_is_left_alone() {
        let raw = "+++\n+++\n\n```md\n[甲](/posts/a/)\n```\n\n[乙](/posts/a/)\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 1, "围栏里的那条不该算");
        assert!(out.contains("```md\n[甲](/posts/a/)\n```"), "{out}");
        assert!(out.contains("[乙](/notes/a/)"), "{out}");
    }

    #[test]
    fn long_fence_is_closed_only_by_long_enough_marker() {
        let raw = "+++\n+++\n\n````\n```\n[甲](/posts/a/)\n```\n````\n\n[乙](/posts/a/)\n";
        let (_, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 1);
    }

    #[test]
    fn front_matter_is_never_touched() {
        // aliases 里记的正是旧地址，改掉就等于把刚建好的重定向拆了
        let raw = "+++\naliases = [\"/posts/a/\"]\n+++\n\n[甲](/posts/a/)\n";
        let (out, hits) = rewritten(raw, &moves("/posts/a/", "/notes/a/"));
        assert_eq!(hits, 1);
        assert!(out.contains("aliases = [\"/posts/a/\"]"), "{out}");
    }

    #[test]
    fn prose_mentions_at_line_start_are_not_links() {
        let raw = "+++\n+++\n\n/posts/a/ 这个地址以前是甲。\n";
        assert!(rewrite(raw, &moves("/posts/a/", "/notes/a/"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn no_hit_means_no_write() {
        let raw = "+++\n+++\n\n没有任何链接。\n";
        assert!(rewrite(raw, &moves("/posts/a/", "/notes/a/"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn empty_moves_short_circuit() {
        assert!(rewrite("+++\n+++\n\n[甲](/posts/a/)\n", &[])
            .unwrap()
            .is_none());
    }

    #[test]
    fn one_pass_only_no_chain_rewrite() {
        // 甲搬到乙的旧位置这种情况下，不能让甲的新地址又被乙那条规则改一次
        let moves = vec![
            UrlMove {
                from: "/posts/a/".to_string(),
                to: "/posts/b/".to_string(),
            },
            UrlMove {
                from: "/posts/b/".to_string(),
                to: "/notes/b/".to_string(),
            },
        ];
        let raw = "+++\n+++\n\n[甲](/posts/a/) [乙](/posts/b/)\n";
        let (out, hits) = rewritten(raw, &moves);
        assert_eq!(hits, 2);
        assert!(out.contains("[甲](/posts/b/)"), "{out}");
        assert!(out.contains("[乙](/notes/b/)"), "{out}");
    }

    #[test]
    fn broken_front_matter_is_an_error_not_a_guess() {
        let raw = "+++\ntitle = \"甲\"\n\n[甲](/posts/a/)\n";
        assert!(rewrite(raw, &moves("/posts/a/", "/notes/a/")).is_err());
    }

    // ------------------------------------------------------------ 相对链接

    #[test]
    fn relative_targets_resolve_against_the_referring_page() {
        // 页面地址本身就是目录（`/posts/b/`），所以 `../a/` 落在 /posts/a/
        assert_eq!(
            resolve_relative("/posts/b/", "../a/").as_deref(),
            Some("/posts/a/")
        );
        assert_eq!(
            resolve_relative("/posts/b/", "a/").as_deref(),
            Some("/posts/b/a/")
        );
        assert_eq!(
            resolve_relative("/posts/b/", "./a/").as_deref(),
            Some("/posts/b/a/")
        );
        assert_eq!(
            resolve_relative("/a/b/c/", "../../x/").as_deref(),
            Some("/a/x/")
        );
        // 走出站点根就当它不是站内链接，别猜
        assert_eq!(resolve_relative("/posts/b/", "../../../x/"), None);
        // 不带尾斜杠的写法也要认，解析结果跟着不带
        assert_eq!(
            resolve_relative("/posts/b/", "../a").as_deref(),
            Some("/posts/a")
        );
    }

    #[test]
    fn relative_links_pointing_at_the_moved_page_are_counted() {
        let raw = concat!(
            "+++\ntitle = \"乙\"\n+++\n\n",
            "见 [甲](../a/) 与 <a href=\"../a\">甲</a>\n",
            "同栏目的 [丙](./c/)\n",
        );
        // 从 /posts/b/ 看，../a/ 就是 /posts/a/
        let hits = relative_hits(raw, "/posts/b/", &moves("/posts/a/", "/notes/a/")).unwrap();
        assert_eq!(hits, 2, "两种写法都算，指向别处的 ./c/ 不算");
    }

    #[test]
    fn absolute_and_external_and_anchors_are_not_relative_links() {
        let raw = concat!(
            "+++\n+++\n\n",
            "[绝对](/posts/a/) [站外](https://example.com/posts/a/)\n",
            "[锚点](#a) [邮件](mailto:a@example.com) [查询](?tag=a)\n",
        );
        assert_eq!(
            relative_hits(raw, "/posts/b/", &moves("/posts/a/", "/notes/a/")).unwrap(),
            0
        );
    }

    #[test]
    fn relative_links_inside_fenced_code_are_not_counted() {
        let raw = "+++\n+++\n\n```md\n[甲](../a/)\n```\n\n[乙](../a/)\n";
        assert_eq!(
            relative_hits(raw, "/posts/b/", &moves("/posts/a/", "/notes/a/")).unwrap(),
            1
        );
    }

    #[test]
    fn a_page_can_point_at_the_moved_page_with_a_parent_hop() {
        // 根目录的文章用 posts/a/ 指过去（不带 ./ 也不带 ../）
        let raw = "+++\n+++\n\n[甲](posts/a/)\n";
        assert_eq!(
            relative_hits(raw, "/", &moves("/posts/a/", "/notes/a/")).unwrap(),
            1
        );
    }
}
