//! 跨文件查找替换：一次改掉散在几十篇里的同一个说法。
//!
//! 为什么单独一个模块而不是塞进 [`crate::batch`]：批量动作改的是 front matter
//! 的键，靠 `toml_edit` 保序改写；这里改的是**正文文本**，风险完全不同——
//! 改错一个词不会让文件解析失败，只会安静地把内容改坏。
//!
//! 四条自我约束：
//!
//! 1. **只动正文，front matter 一个字节都不碰。** 标题里的错字改起来一眼可见，
//!    而在 TOML 区域做纯文本替换会撞上引号与转义（把 `"` 换掉就毁了整份 front matter）。
//!    要改字段就走批量动作或属性面板，那条路是保序改写。
//! 2. **纯文本，不做正则。** 输入框里的正则最容易写出「本想改一个词、实际扫掉半篇」，
//!    而且干跑看起来正常、命中却在别处。需要正则的人手里有 `sed`。
//! 3. **先干跑再落盘，且两者共用同一份判断**（[`plan_one`]）。
//!    「预览说改三处、执行改了五处」这种事一旦发生，用户再也不会信这个功能。
//! 4. **逐篇独立**：一篇读不出来不影响其余，结果里说清哪篇为什么没改。

use serde::{Deserialize, Serialize};

use crate::config::{ProjectPaths, SourceFormat};
use crate::content::{self, Page};
use crate::error::{Error, Result};
use crate::frontmatter;

/// 每篇最多回传几行示例。
///
/// 预览是给人读的：一篇里命中 40 处时，逐行列出只会把面板刷满而不增加信息，
/// 「这篇 40 处，头几行长这样」已经够判断要不要执行。总数另外单独给。
const LINES_PER_FILE: usize = 5;

/// 一次替换要做什么。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub find: String,
    /// 替换成什么。允许空串——「删掉这个词」是常见诉求。
    pub replace: String,
    /// 忽略大小写。默认关：默认值必须是那个更保守的选项。
    pub ignore_case: bool,
}

/// 要在哪些文件里找。
#[derive(Debug, Clone)]
pub enum Scope {
    /// 全站内容文件。
    All,
    /// 只在这几篇里（相对 `content/` 的源路径）。
    Only(Vec<String>),
}

/// 命中的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineHit {
    /// 正文里的行号，从 1 起。**不是**整份源文的行号：front matter 不在替换范围内，
    /// 用整份源文的行号反而会让人以为 front matter 也会被改。
    pub line: usize,
    pub before: String,
    pub after: String,
}

/// 一篇的替换结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    pub source: String,
    /// 这篇命中几处。
    pub hits: usize,
    /// 头几行示例，最多 [`LINES_PER_FILE`] 行。
    pub lines: Vec<LineHit>,
}

/// 跳过的一篇，以及为什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub source: String,
    pub reason: String,
}

/// 干跑或执行的结果。
///
/// 干跑与执行用同一个结构：面板与命令行不必为两种形状写两套渲染，
/// 「执行完的样子」和「刚才预览的样子」也就能逐行对上。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// 有命中的文件（干跑时是「会改」，执行后是「已改」）。
    pub files: Vec<FileChange>,
    /// 命中总处数。
    pub hits: usize,
    pub skipped: Vec<Skipped>,
}

/// 干跑：算出每篇会改哪几处，不碰磁盘。
pub fn preview(paths: &ProjectPaths, scope: &Scope, rule: &Rule) -> Result<Report> {
    run(paths, scope, rule, false)
}

/// 执行：把干跑算出来的改动写回磁盘。
///
/// 界面与命令行都该先让人看过 [`preview`] 再调这里——正文替换没有撤销栈。
pub fn apply(paths: &ProjectPaths, scope: &Scope, rule: &Rule) -> Result<Report> {
    run(paths, scope, rule, true)
}

fn run(paths: &ProjectPaths, scope: &Scope, rule: &Rule, write: bool) -> Result<Report> {
    let find = check(rule)?;
    let mut out = Report::default();

    for source in sources_in(paths, scope)? {
        let path = content::resolve_source(&paths.content, &source);
        if !content::is_within(&paths.content, &path) {
            out.skipped.push(Skipped {
                source,
                reason: "不在内容目录内".to_string(),
            });
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                out.skipped.push(Skipped {
                    source,
                    reason: err.to_string(),
                });
                continue;
            }
        };
        match plan_one(&source, &raw, find, rule) {
            Err(err) => out.skipped.push(Skipped {
                source,
                reason: err.to_string(),
            }),
            Ok(None) => {}
            Ok(Some((change, updated))) => {
                if write {
                    if let Err(err) = std::fs::write(&path, &updated) {
                        out.skipped.push(Skipped {
                            source,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                }
                out.hits += change.hits;
                out.files.push(change);
            }
        }
    }
    Ok(out)
}

/// 一篇的替换计划：命中摘要 + 替换后的完整源文。`None` 表示这篇没有命中。
///
/// 干跑与执行都走这里，所以「改哪几处」只有一份定义。
fn plan_one(
    source: &str,
    raw: &str,
    find: &str,
    rule: &Rule,
) -> Result<Option<(FileChange, String)>> {
    let start = frontmatter::body_start(raw)?;
    let (head, body) = raw.split_at(start);

    let mut hits = 0usize;
    let mut lines = Vec::new();
    let mut new_body = String::with_capacity(body.len());
    // 按行走：既能给出行号与前后对照，也顺手保证替换不会跨行发生
    for (number, line) in body.split_inclusive('\n').enumerate() {
        let count = count_in(line, find, rule.ignore_case);
        if count == 0 {
            new_body.push_str(line);
            continue;
        }
        let replaced = replace_in(line, find, &rule.replace, rule.ignore_case);
        hits += count;
        if lines.len() < LINES_PER_FILE {
            lines.push(LineHit {
                line: number + 1,
                before: line.trim_end_matches(['\r', '\n']).to_string(),
                after: replaced.trim_end_matches(['\r', '\n']).to_string(),
            });
        }
        new_body.push_str(&replaced);
    }
    if hits == 0 {
        return Ok(None);
    }
    Ok(Some((
        FileChange {
            source: source.to_string(),
            hits,
            lines,
        },
        format!("{head}{new_body}"),
    )))
}

/// 校验规则，顺手给出用于比对的查找串（忽略大小写时是小写形式）。
fn check(rule: &Rule) -> Result<&str> {
    if rule.find.is_empty() {
        return Err(Error::Other("要查找的内容不能为空".to_string()));
    }
    if rule.find.contains('\n') {
        return Err(Error::Other(
            "查找的内容不能跨行：跨行替换在预览里看不出真实效果".to_string(),
        ));
    }
    if rule.find == rule.replace {
        return Err(Error::Other(
            "查找与替换内容相同，不会有任何变化".to_string(),
        ));
    }
    Ok(&rule.find)
}

/// 这一行里出现几次。不重叠计数（`aa` 在 `aaa` 里算一次），与替换行为一致。
fn count_in(line: &str, find: &str, ignore_case: bool) -> usize {
    if ignore_case {
        lower(line).matches(&lower(find)).count()
    } else {
        line.matches(find).count()
    }
}

/// 替换这一行。
///
/// 忽略大小写时不能直接用 `str::replace`：它按字节比，`Meta` 不会命中 `meta`。
/// 这里在小写副本上定位、在原文上切割，被替换掉的那段原始大小写自然消失，
/// 未命中的部分逐字保留。
fn replace_in(line: &str, find: &str, to: &str, ignore_case: bool) -> String {
    if !ignore_case {
        return line.replace(find, to);
    }
    let haystack = lower(line);
    let needle = lower(find);
    let mut out = String::with_capacity(line.len());
    let mut at = 0usize;
    while let Some(found) = haystack[at..].find(&needle) {
        let from = at + found;
        // 小写化可能改变字节长度（如 `İ`），此时无法把偏移安全地映射回原文，
        // 只好放弃这一行的替换而不是切出半个字符
        if !line.is_char_boundary(from) || !line.is_char_boundary(from + needle.len()) {
            return line.to_string();
        }
        out.push_str(&line[at..from]);
        out.push_str(to);
        at = from + needle.len();
    }
    out.push_str(&line[at..]);
    out
}

fn lower(text: &str) -> String {
    text.to_lowercase()
}

/// 要处理的源路径清单。
///
/// 全站那一档走 `load_all`：收哪些扩展名的规则只该有一处，
/// 自己再写一遍目录遍历就会出现「界面能替换、命令行漏掉 .html」。
/// 这里只用它给出的路径，不看渲染后的正文，所以固定用默认格式。
fn sources_in(paths: &ProjectPaths, scope: &Scope) -> Result<Vec<String>> {
    match scope {
        Scope::Only(sources) => Ok(sources.clone()),
        Scope::All => Ok(content::load_all(&paths.content, SourceFormat::default())?
            .into_iter()
            .map(|page: Page| page.source)
            .collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Assets, Build};

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: ProjectPaths,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::new(dir.path(), &Build::default(), &Assets::default());
        std::fs::create_dir_all(&paths.content).unwrap();
        Fixture { _dir: dir, paths }
    }

    fn write(f: &Fixture, source: &str, raw: &str) {
        let path = content::resolve_source(&f.paths.content, source);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    fn read(f: &Fixture, source: &str) -> String {
        std::fs::read_to_string(content::resolve_source(&f.paths.content, source)).unwrap()
    }

    fn rule(find: &str, to: &str) -> Rule {
        Rule {
            find: find.to_string(),
            replace: to.to_string(),
            ignore_case: false,
        }
    }

    #[test]
    fn front_matter_is_never_touched() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"旧名\"\ntags = [\"旧名\"]\n+++\n\n正文提到旧名两次：旧名。\n",
        );

        let out = apply(
            &f.paths,
            &Scope::Only(vec!["posts/a.md".into()]),
            &rule("旧名", "新名"),
        )
        .unwrap();
        assert_eq!(out.hits, 2, "{out:?}");

        let after = read(&f, "posts/a.md");
        assert!(after.contains("title = \"旧名\""), "标题不能被动：{after}");
        assert!(after.contains("tags = [\"旧名\"]"), "标签不能被动：{after}");
        assert!(after.contains("正文提到新名两次：新名。"), "{after}");
    }

    #[test]
    fn preview_and_apply_agree() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n\n甲说乙。\n");
        write(
            &f,
            "posts/b.md",
            "+++\ntitle = \"乙\"\n+++\n\n没有那个词。\n",
        );

        let scope = Scope::All;
        let dry = preview(&f.paths, &scope, &rule("乙", "丙")).unwrap();
        assert_eq!(dry.hits, 1, "{dry:?}");
        assert_eq!(dry.files.len(), 1, "没命中的文件不该出现在清单里");
        assert_eq!(dry.files[0].source, "posts/a.md");
        assert_eq!(dry.files[0].lines[0].line, 1, "行号从正文算起");
        assert_eq!(dry.files[0].lines[0].before, "甲说乙。");
        assert_eq!(dry.files[0].lines[0].after, "甲说丙。");
        // 干跑不写盘
        assert!(read(&f, "posts/a.md").contains("甲说乙。"));

        let done = apply(&f.paths, &scope, &rule("乙", "丙")).unwrap();
        assert_eq!(done.hits, dry.hits);
        assert_eq!(done.files, dry.files, "执行结果要与预览逐字一致");
        assert!(read(&f, "posts/a.md").contains("甲说丙。"));
    }

    #[test]
    fn ignore_case_keeps_the_rest_of_the_line_verbatim() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\n+++\n\nMeta 与 meta 与 META，还有 Metadata。\n",
        );

        let out = apply(
            &f.paths,
            &Scope::Only(vec!["posts/a.md".into()]),
            &Rule {
                find: "meta".into(),
                replace: "元".into(),
                ignore_case: true,
            },
        )
        .unwrap();
        assert_eq!(out.hits, 4, "Metadata 里那一处也算：纯文本替换不认词边界");
        assert_eq!(
            read(&f, "posts/a.md").lines().nth(4).unwrap(),
            "元 与 元 与 元，还有 元data。"
        );
    }

    #[test]
    fn empty_replacement_deletes_the_word() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\n+++\n\n请（务必）看。\n",
        );

        let out = apply(
            &f.paths,
            &Scope::Only(vec!["posts/a.md".into()]),
            &rule("（务必）", ""),
        )
        .unwrap();
        assert_eq!(out.hits, 1);
        assert!(read(&f, "posts/a.md").contains("请看。"));
    }

    #[test]
    fn bad_rules_are_refused_before_touching_anything() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n\n正文。\n");
        let scope = Scope::All;

        assert!(preview(&f.paths, &scope, &rule("", "x"))
            .unwrap_err()
            .to_string()
            .contains("不能为空"));
        assert!(preview(&f.paths, &scope, &rule("正\n文", "x"))
            .unwrap_err()
            .to_string()
            .contains("不能跨行"));
        assert!(preview(&f.paths, &scope, &rule("正文", "正文"))
            .unwrap_err()
            .to_string()
            .contains("不会有任何变化"));
    }

    #[test]
    fn one_bad_file_does_not_abort_the_run() {
        let f = fixture();
        write(&f, "posts/ok.md", "+++\ntitle = \"好的\"\n+++\n\n旧名\n");
        // 缺结束围栏：这一篇要报原因，另一篇照常改
        write(&f, "posts/broken.md", "+++\ntitle = \"坏的\"\n\n旧名\n");

        let out = apply(
            &f.paths,
            &Scope::Only(vec![
                "posts/broken.md".into(),
                "posts/ok.md".into(),
                "../../etc/passwd".into(),
            ]),
            &rule("旧名", "新名"),
        )
        .unwrap();
        assert_eq!(out.files.len(), 1, "{out:?}");
        assert_eq!(out.files[0].source, "posts/ok.md");
        assert_eq!(out.skipped.len(), 2, "{out:?}");
        assert!(out.skipped.iter().any(|s| s.reason.contains("+++")));
        assert!(out
            .skipped
            .iter()
            .any(|s| s.reason.contains("不在内容目录内")));
        assert!(
            read(&f, "posts/broken.md").contains("旧名"),
            "坏文件不能被写"
        );
    }

    #[test]
    fn line_samples_are_capped_but_the_count_is_not() {
        let f = fixture();
        let body: String = (0..LINES_PER_FILE + 3).map(|_| "旧名\n").collect();
        write(
            &f,
            "posts/a.md",
            &format!("+++\ntitle = \"甲\"\n+++\n\n{body}"),
        );

        let out = preview(
            &f.paths,
            &Scope::Only(vec!["posts/a.md".into()]),
            &rule("旧名", "新名"),
        )
        .unwrap();
        assert_eq!(out.hits, LINES_PER_FILE + 3, "总数要给全");
        assert_eq!(out.files[0].lines.len(), LINES_PER_FILE, "示例行要有上限");
    }

    #[test]
    fn windows_line_endings_survive() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\r\ntitle = \"甲\"\r\n+++\r\n\r\n旧名在这里\r\n第二行\r\n",
        );

        apply(
            &f.paths,
            &Scope::Only(vec!["posts/a.md".into()]),
            &rule("旧名", "新名"),
        )
        .unwrap();
        let after = read(&f, "posts/a.md");
        assert!(after.contains("新名在这里\r\n第二行\r\n"), "{after:?}");
        assert!(
            after.starts_with("+++\r\ntitle = \"甲\"\r\n+++\r\n"),
            "{after:?}"
        );
    }
}
