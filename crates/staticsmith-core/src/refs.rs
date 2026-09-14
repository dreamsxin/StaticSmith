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

use serde::Serialize;

use crate::error::Result;
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
}
