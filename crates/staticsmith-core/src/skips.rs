//! 跳过汇总：一次批量动作里「哪几篇没动、为什么」。
//!
//! 批量改的是磁盘上的源文，而这个项目**不做撤销栈**（理由见 `batch` 模块），
//! 所以这份汇总是用户判断「要不要去 Git 里回滚」的全部依据。它必须同时满足两件
//! 互相拉扯的要求：一行之内说清全局（气泡、命令行的头一行），以及事后查得到每一篇。
//!
//! 放在 core 里而不是各前端各写一份：同一个动作在界面、命令行、MCP 三个入口
//! 说法不一样，是「专业工具」最不该有的毛病。前端那份 `src/skips.ts` 与这里对齐，
//! 两边的措辞由同名测试各自钉住。

use serde::Serialize;

/// 跳过的一篇，以及为什么。
///
/// `batch`、`replace`、`import` 三处原先各定义了一份一模一样的结构；
/// 现在共用这一个（字段没变，序列化出来的 JSON 也没变）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub source: String,
    pub reason: String,
}

impl Skipped {
    pub fn new(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            reason: reason.into(),
        }
    }
}

/// 一行里最多列几种原因。再多就成了没人读的一长串，剩下的给个数、明细里有全部。
const MAX_REASONS: usize = 3;

/// 汇总结果。
pub struct Summary {
    /// 一行汇总。没有跳过时是空串。
    pub line: String,
    /// 每一篇一行（`源文件：原因`）。
    pub details: Vec<String>,
}

/// 把跳过清单归成一句话。
///
/// 只报第一条原因加「另有 N 篇」是不够的：五篇里有两种原因时，第二种从没露过面，
/// 而用户会按看到的那一条去排查——查错方向比不给信息更费时间。
pub fn summarize(skipped: &[Skipped]) -> Summary {
    let details: Vec<String> = skipped
        .iter()
        .map(|item| format!("{}：{}", item.source, item.reason))
        .collect();

    if skipped.is_empty() {
        return Summary {
            line: String::new(),
            details,
        };
    }

    // 一篇的时候报出是哪一篇：一篇是找得到的，不必只给数量
    if skipped.len() == 1 {
        return Summary {
            line: format!("跳过 1 篇：{}「{}」", skipped[0].source, skipped[0].reason),
            details,
        };
    }

    // 按第一次出现的顺序记数，再按篇数稳定排序：同样多的两种原因，顺序不会随机
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for item in skipped {
        match counts.iter_mut().find(|(reason, _)| *reason == item.reason) {
            Some((_, count)) => *count += 1,
            None => counts.push((&item.reason, 1)),
        }
    }

    // 都是同一个原因时说出来：只报「跳过 3 篇」会让人以为还有别的情况没露面
    if counts.len() == 1 {
        return Summary {
            line: format!("跳过 {} 篇，都是「{}」", skipped.len(), skipped[0].reason),
            details,
        };
    }

    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    let listed: Vec<String> = counts
        .iter()
        .take(MAX_REASONS)
        .map(|(reason, count)| format!("{count} 篇「{reason}」"))
        .collect();
    let rest = if counts.len() > MAX_REASONS {
        format!("，另有 {} 种原因", counts.len() - MAX_REASONS)
    } else {
        String::new()
    };

    Summary {
        line: format!("跳过 {} 篇：{}{}", skipped.len(), listed.join("、"), rest),
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_skipped_says_nothing() {
        let out = summarize(&[]);
        assert_eq!(out.line, "");
        assert!(out.details.is_empty());
    }

    #[test]
    fn a_single_skip_names_the_file() {
        let out = summarize(&[Skipped::new("posts/a.md", "front matter 读不出来")]);
        assert_eq!(out.line, "跳过 1 篇：posts/a.md「front matter 读不出来」");
    }

    #[test]
    fn one_reason_for_all_of_them_is_said_out_loud() {
        let out = summarize(&[
            Skipped::new("posts/a.md", "找不到这篇内容"),
            Skipped::new("posts/b.md", "找不到这篇内容"),
            Skipped::new("posts/c.md", "找不到这篇内容"),
        ]);
        assert_eq!(out.line, "跳过 3 篇，都是「找不到这篇内容」");
    }

    #[test]
    fn several_reasons_are_listed_by_count() {
        let out = summarize(&[
            Skipped::new("posts/a.md", "已经在这个栏目里"),
            Skipped::new("posts/b.md", "找不到这篇内容"),
            Skipped::new("posts/c.md", "找不到这篇内容"),
        ]);
        assert_eq!(
            out.line,
            "跳过 3 篇：2 篇「找不到这篇内容」、1 篇「已经在这个栏目里」"
        );
    }

    #[test]
    fn too_many_kinds_of_reason_get_capped() {
        let out = summarize(&[
            Skipped::new("a.md", "甲"),
            Skipped::new("b.md", "甲"),
            Skipped::new("c.md", "乙"),
            Skipped::new("d.md", "丙"),
            Skipped::new("e.md", "丁"),
            Skipped::new("f.md", "戊"),
        ]);
        assert_eq!(
            out.line,
            "跳过 6 篇：2 篇「甲」、1 篇「乙」、1 篇「丙」，另有 2 种原因"
        );
    }

    #[test]
    fn every_file_is_in_the_details() {
        let out = summarize(&[
            Skipped::new("posts/a.md", "目标已存在"),
            Skipped::new("posts/b.md", "找不到这篇内容"),
        ]);
        assert_eq!(
            out.details,
            vec!["posts/a.md：目标已存在", "posts/b.md：找不到这篇内容"]
        );
    }

    /// 前端 `src/skips.ts` 与这里说的是同一句话。两处各有测试，措辞改一边就会红。
    #[test]
    fn wording_matches_the_frontend_helper() {
        let out = summarize(&[
            Skipped::new("posts/a.md", "目标已存在"),
            Skipped::new("posts/b.md", "front matter 读不出来"),
            Skipped::new("posts/c.md", "front matter 读不出来"),
        ]);
        assert_eq!(
            out.line,
            "跳过 3 篇：2 篇「front matter 读不出来」、1 篇「目标已存在」"
        );
    }
}
