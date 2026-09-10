//! 批量动作：一次改多篇。
//!
//! 运营里最费手的不是写，而是「把这十二篇都补上标签」「把这一批从草稿放出去」
//! 「这几篇挪到新栏目」。逐篇点开、改、保存，人会在第五篇上开始出错。
//!
//! 三条自我约束：
//!
//! 1. **逐篇独立**。一篇失败不影响其余，结果里分「改了哪些」与「跳过哪些及原因」，
//!    而不是整批回滚——批量改的多是互不相干的文章，为了一篇没权限的文件把另外
//!    十一篇的成果扔掉更糟。
//! 2. **只动该动的键**。走 `frontmatter` 的保序改写，正文、注释、其他键不受影响。
//! 3. **搬动即补旧地址**。移动文章会改 URL，默认写入 `aliases`，
//!    构建后老链接经重定向页继续可用。

use std::path::Path;

use serde::Serialize;

use crate::config::{ProjectPaths, SourceFormat};
use crate::content::{self, Page};
use crate::error::{Error, Result};
use crate::frontmatter::{self, Patch};
use crate::util;

/// 加载全站页面，只为读 front matter 与路径。
///
/// 批量动作与栏目改名关心的是「这一篇叫什么、在哪、标签是什么」，从不看渲染后的正文，
/// 所以这里固定用默认格式，不必把站点的 `source_format` 一路传进来。
/// 之所以安全：`load_all` 收哪些扩展名与格式无关，HTML 站点的 `.html` 文件同样在列，
/// 不会出现「搬动或改名时漏掉一篇、它的旧地址没人补」。
fn load_for_front_matter(content_root: &Path) -> Result<Vec<Page>> {
    content::load_all(content_root, SourceFormat::default())
}

/// 跳过的一篇，以及为什么。
///
/// 「为什么」必须回传：批量动作最怕的是「点了没反应」，用户需要知道是自己选错了
/// 还是程序没做。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub source: String,
    pub reason: String,
}

/// 批量改 front matter 的结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    /// 真正写了盘的源文件。
    pub changed: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// 一次搬动。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
    /// 是否补了旧地址。目标 URL 与原来相同（只换目录不换 slug 不会发生）时为假。
    pub alias_added: bool,
}

/// 批量搬动的结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct MoveOutcome {
    pub moved: Vec<Moved>,
    pub skipped: Vec<Skipped>,
}

/// 标签增删。
///
/// 用「加什么、去什么」而不是「新标签集合」：批量场景里各篇原有标签不同，
/// 整集合覆盖会把别的标签洗掉。
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct TagEdit {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// 批量增删标签。原有顺序保留，新标签追加在后面。
pub fn edit_tags(paths: &ProjectPaths, sources: &[String], edit: &TagEdit) -> Result<Outcome> {
    let (add, remove) = clean_tag_edit(edit)?;
    each(paths, sources, |raw| {
        Ok(tags_after(raw, &add, &remove)?.map(|tags| Patch {
            tags: Some(tags),
            ..Patch::default()
        }))
    })
}

/// 清洗并校验标签增删：两边都空就是没让人干活，直接报错而不是静默无事发生。
fn clean_tag_edit(edit: &TagEdit) -> Result<(Vec<String>, Vec<String>)> {
    let clean = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    };
    let add = clean(&edit.add);
    let remove = clean(&edit.remove);
    if add.is_empty() && remove.is_empty() {
        return Err(Error::Other("没有要增删的标签".to_string()));
    }
    Ok((add, remove))
}

/// 增删之后的标签集合。`None` 表示这篇没有变化。
///
/// 预览与真正写盘共用这一份判断——两处各算一遍，迟早会出现「预览说改，执行却跳过」。
fn tags_after(raw: &str, add: &[String], remove: &[String]) -> Result<Option<Vec<String>>> {
    let before = frontmatter::read(raw)?.tags;
    let mut tags = before.clone();
    tags.retain(|t| !remove.contains(t));
    for tag in add {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    Ok(if tags == before { None } else { Some(tags) })
}

/// 批量设置草稿开关。已经是目标状态的会被跳过，不会白写一次盘。
pub fn set_draft(paths: &ProjectPaths, sources: &[String], draft: bool) -> Result<Outcome> {
    each(paths, sources, move |raw| {
        if frontmatter::read(raw)?.draft == draft {
            return Ok(None);
        }
        Ok(Some(Patch {
            draft: Some(draft),
            ..Patch::default()
        }))
    })
}

/// 批量搬到另一个栏目。
///
/// `keep_aliases` 默认由调用方决定，但界面与 Agent 都该给真：搬动会改 URL，
/// 不补旧地址就等于把外部链接全部打断。
pub fn move_to_section(
    paths: &ProjectPaths,
    sources: &[String],
    to_section: &str,
    keep_aliases: bool,
) -> Result<MoveOutcome> {
    let target = util::sanitize_relative_dir(to_section);
    let pages = load_for_front_matter(&paths.content)?;
    let mut out = MoveOutcome::default();

    for source in sources {
        let Some(page) = pages.iter().find(|p| &p.source == source) else {
            out.skipped.push(Skipped {
                source: source.clone(),
                reason: "找不到这篇内容".to_string(),
            });
            continue;
        };
        match move_one(paths, page, &target, keep_aliases) {
            Ok(Some(moved)) => out.moved.push(moved),
            Ok(None) => out.skipped.push(Skipped {
                source: source.clone(),
                reason: "已经在这个栏目里".to_string(),
            }),
            Err(err) => out.skipped.push(Skipped {
                source: source.clone(),
                reason: err.to_string(),
            }),
        }
    }
    Ok(out)
}

/// 搬一篇的判断结果。预览与执行共用，避免「预览说能搬、执行却跳过」。
enum MoveDecision {
    /// 本来就在目标栏目。
    Same,
    /// 不能搬，附原因。
    Blocked(String),
    Go {
        new_source: String,
        old_url: String,
    },
}

fn move_decision(paths: &ProjectPaths, page: &Page, target: &str) -> MoveDecision {
    if page.section == target {
        return MoveDecision::Same;
    }
    // 栏目索引页搬走会让原栏目失去列表页，同时在新栏目里撞上人家的索引页
    if page.is_index {
        return MoveDecision::Blocked("栏目索引页不能搬动，请改栏目名或新建栏目".to_string());
    }
    let Some(file_name) = page.source.rsplit('/').next() else {
        return MoveDecision::Blocked(format!("源路径异常: {}", page.source));
    };
    let new_source = if target.is_empty() {
        file_name.to_string()
    } else {
        format!("{target}/{file_name}")
    };
    // 目标路径由用户填的栏目名拼出来，越界要在干跑阶段就说清楚，而不是等落盘时报错。
    match content::resolve_source(&paths.content, &new_source) {
        Err(err) => return MoveDecision::Blocked(err.to_string()),
        Ok(path) if path.exists() => {
            return MoveDecision::Blocked(format!("{new_source} 已存在同名文件"))
        }
        Ok(_) => {}
    }
    MoveDecision::Go {
        new_source,
        old_url: page.url.clone(),
    }
}

/// 搬一篇。返回 `None` 表示本来就在目标栏目。
fn move_one(
    paths: &ProjectPaths,
    page: &Page,
    target: &str,
    keep_aliases: bool,
) -> Result<Option<Moved>> {
    let (new_source, old_url) = match move_decision(paths, page, target) {
        MoveDecision::Same => return Ok(None),
        MoveDecision::Blocked(reason) => return Err(Error::Other(reason)),
        MoveDecision::Go {
            new_source,
            old_url,
        } => (new_source, old_url),
    };

    let from_path = content::resolve_source(&paths.content, &page.source)?;
    let to_path = content::resolve_source(&paths.content, &new_source)?;
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::rename(&from_path, &to_path).map_err(|e| Error::io(&from_path, e))?;

    let mut alias_added = false;
    if keep_aliases {
        alias_added = content::add_alias(&to_path, &old_url)?;
    }
    Ok(Some(Moved {
        from: page.source.clone(),
        to: new_source,
        alias_added,
    }))
}

/// 批量删除。不可逆，界面必须先二次确认。
pub fn delete(paths: &ProjectPaths, sources: &[String]) -> Result<Outcome> {
    let mut out = Outcome::default();
    for source in sources {
        let path = match content::resolve_source(&paths.content, source) {
            Ok(path) => path,
            Err(err) => {
                out.skipped.push(Skipped {
                    source: source.clone(),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        match std::fs::remove_file(&path) {
            Ok(()) => out.changed.push(source.clone()),
            Err(err) => out.skipped.push(Skipped {
                source: source.clone(),
                reason: err.to_string(),
            }),
        }
    }
    Ok(out)
}

/// 要预览的动作。
///
/// 与执行用的四个函数一一对应，判断逻辑共用，避免「预览说改、执行却跳过」。
#[derive(Debug, Clone)]
pub enum Action {
    Tags(TagEdit),
    Draft(bool),
    Move { to_section: String },
    Delete,
}

/// 预览里的一条：这篇会发生什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub source: String,
    /// 是否真的会改动。为假时 `effect` 说明为什么不动。
    pub changes: bool,
    /// 人能读的一句话：「加标签 运营」「搬到 notes/」「已经是草稿」。
    pub effect: String,
}

/// 干跑结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Preview {
    pub changes: Vec<Change>,
    /// 真的会改动的篇数。
    pub affected: usize,
}

/// 干跑：算出每篇会发生什么，不碰磁盘。
///
/// 只给「不可逆或会改地址」的动作用（搬动、删除）就够了——加标签、切草稿这类
/// 反手就能改回来的动作，多一步确认只是白点一下。
pub fn preview(paths: &ProjectPaths, sources: &[String], action: &Action) -> Result<Preview> {
    let mut out = Preview::default();
    let pages = match action {
        Action::Move { .. } => load_for_front_matter(&paths.content)?,
        _ => Vec::new(),
    };
    let tag_edit = match action {
        Action::Tags(edit) => Some(clean_tag_edit(edit)?),
        _ => None,
    };
    let target = match action {
        Action::Move { to_section } => util::sanitize_relative_dir(to_section),
        _ => String::new(),
    };

    for source in sources {
        let path = match content::resolve_source(&paths.content, source) {
            Ok(path) => path,
            Err(err) => {
                out.changes.push(Change {
                    source: source.clone(),
                    changes: false,
                    effect: err.to_string(),
                });
                continue;
            }
        };

        let change = match action {
            Action::Delete => Change {
                source: source.clone(),
                changes: path.is_file(),
                effect: if path.is_file() {
                    "删除，不可撤销".to_string()
                } else {
                    "文件不存在".to_string()
                },
            },
            Action::Move { .. } => match pages.iter().find(|p| &p.source == source) {
                None => Change {
                    source: source.clone(),
                    changes: false,
                    effect: "找不到这篇内容".to_string(),
                },
                Some(page) => match move_decision(paths, page, &target) {
                    MoveDecision::Same => Change {
                        source: source.clone(),
                        changes: false,
                        effect: "已经在这个栏目里".to_string(),
                    },
                    MoveDecision::Blocked(reason) => Change {
                        source: source.clone(),
                        changes: false,
                        effect: reason,
                    },
                    MoveDecision::Go {
                        new_source,
                        old_url,
                    } => Change {
                        source: source.clone(),
                        changes: true,
                        effect: format!("搬到 {new_source}，旧地址 {old_url}"),
                    },
                },
            },
            Action::Tags(_) | Action::Draft(_) => {
                let raw = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e));
                match raw {
                    Err(err) => Change {
                        source: source.clone(),
                        changes: false,
                        effect: err.to_string(),
                    },
                    Ok(raw) => describe_patch(source, &raw, action, tag_edit.as_ref()),
                }
            }
        };
        if change.changes {
            out.affected += 1;
        }
        out.changes.push(change);
    }
    Ok(out)
}

/// front matter 类动作的一句话说明。
fn describe_patch(
    source: &str,
    raw: &str,
    action: &Action,
    tag_edit: Option<&(Vec<String>, Vec<String>)>,
) -> Change {
    let described = match action {
        Action::Tags(_) => match tag_edit {
            // 调用方一定先 `clean_tag_edit` 再进来，所以正常不可能是 None。
            // 但**不用 `expect`**：这是全仓唯一一处真正的逻辑不变量断言。
            // 桌面端现在能在 IPC 边界兜住 panic，但 CLI 与 MCP 没有这层保护，
            // 一个 expect 会让批量改内容的进程当场中断。
            // 将来若有人加了新入口忘了传，报一行「说不出会改什么」远比 panic 好。
            None => Err(Error::Other(
                "内部错误：标签动作没有带上清洗后的增删表".to_string(),
            )),
            Some((add, remove)) => tags_after(raw, add, remove)
                .map(|after| after.map(|tags| format!("标签改为 {}", tags.join("、")))),
        },
        Action::Draft(draft) => frontmatter::read(raw).map(|fm| {
            if fm.draft == *draft {
                None
            } else if *draft {
                Some("收回为草稿".to_string())
            } else {
                Some("发布".to_string())
            }
        }),
        _ => Ok(None),
    };
    match described {
        Err(err) => Change {
            source: source.to_string(),
            changes: false,
            effect: err.to_string(),
        },
        Ok(None) => Change {
            source: source.to_string(),
            changes: false,
            effect: "已经是目标状态".to_string(),
        },
        Ok(Some(effect)) => Change {
            source: source.to_string(),
            changes: true,
            effect,
        },
    }
}

/// 逐篇读源文、算出补丁、写回。补丁为 `None` 表示这篇无需改动。
fn each(
    paths: &ProjectPaths,
    sources: &[String],
    mut patch_for: impl FnMut(&str) -> Result<Option<Patch>>,
) -> Result<Outcome> {
    let mut out = Outcome::default();
    for source in sources {
        let path = match content::resolve_source(&paths.content, source) {
            Ok(path) => path,
            Err(err) => {
                out.skipped.push(Skipped {
                    source: source.clone(),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| Error::io(&path, e))
            .and_then(|raw| match patch_for(&raw)? {
                Some(patch) => {
                    let updated = frontmatter::apply(&raw, &patch)?;
                    if updated == raw {
                        return Ok(false);
                    }
                    std::fs::write(&path, updated).map_err(|e| Error::io(&path, e))?;
                    Ok(true)
                }
                None => Ok(false),
            });
        match result {
            Ok(true) => out.changed.push(source.clone()),
            Ok(false) => out.skipped.push(Skipped {
                source: source.clone(),
                reason: "已经是目标状态".to_string(),
            }),
            Err(err) => out.skipped.push(Skipped {
                source: source.clone(),
                reason: err.to_string(),
            }),
        }
    }
    Ok(out)
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
        let path = content::resolve_source(&f.paths.content, source).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    fn read(f: &Fixture, source: &str) -> String {
        std::fs::read_to_string(content::resolve_source(&f.paths.content, source).unwrap()).unwrap()
    }

    #[test]
    fn preview_tells_what_would_change_without_touching_disk() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\ndraft = true\n+++\n");
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        write(&f, "notes/a.md", "+++\ntitle = \"同名\"\n+++\n");

        let sources = vec![
            "posts/a.md".to_string(),
            "posts/index.md".to_string(),
            "posts/missing.md".to_string(),
        ];

        // 搬动：一篇能搬（撞名的目标除外）、索引页拒绝、找不到的报出来
        let moving = preview(
            &f.paths,
            &sources,
            &Action::Move {
                to_section: "notes".to_string(),
            },
        )
        .unwrap();
        assert_eq!(moving.affected, 0, "{moving:?}");
        assert!(moving.changes[0].effect.contains("已存在同名文件"));
        assert!(moving.changes[1].effect.contains("索引页不能搬动"));
        assert!(moving.changes[2].effect.contains("找不到"));

        let moving = preview(
            &f.paths,
            &["posts/a.md".to_string()],
            &Action::Move {
                to_section: "essays".to_string(),
            },
        )
        .unwrap();
        assert_eq!(moving.affected, 1);
        assert!(
            moving.changes[0].effect.contains("搬到 essays/a.md"),
            "{:?}",
            moving.changes
        );
        assert!(
            moving.changes[0].effect.contains("/posts/a/"),
            "要说清旧地址"
        );

        // 发布：草稿那篇会变，非草稿的说明「已经是目标状态」
        let publishing = preview(
            &f.paths,
            &["posts/a.md".to_string(), "notes/a.md".to_string()],
            &Action::Draft(false),
        )
        .unwrap();
        assert_eq!(publishing.affected, 1);
        assert_eq!(publishing.changes[0].effect, "发布");
        assert_eq!(publishing.changes[1].effect, "已经是目标状态");

        // 干跑不写盘
        assert!(std::fs::read_to_string(
            content::resolve_source(&f.paths.content, "posts/a.md").unwrap()
        )
        .unwrap()
        .contains("draft = true"));
        assert!(!content::resolve_source(&f.paths.content, "essays/a.md")
            .unwrap()
            .exists());
    }

    #[test]
    fn preview_and_apply_agree_on_tags() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\ntags = [\"模板\"]\n+++\n",
        );
        write(
            &f,
            "posts/b.md",
            "+++\ntitle = \"乙\"\ntags = [\"模板\"]\n+++\n",
        );

        let sources = vec!["posts/a.md".to_string(), "posts/b.md".to_string()];
        let edit = TagEdit {
            add: vec!["模板".into()],
            remove: Vec::new(),
        };
        // 两篇都已经有这个标签：预览说不会变，执行也该一篇都不改
        let dry = preview(&f.paths, &sources, &Action::Tags(edit.clone())).unwrap();
        assert_eq!(dry.affected, 0, "{dry:?}");

        let out = edit_tags(&f.paths, &sources, &edit).unwrap();
        assert!(out.changed.is_empty());
        assert_eq!(out.skipped.len(), 2);
    }

    #[test]
    fn tags_are_merged_not_overwritten() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\ntags = [\"模板\"]\n+++\n正文\n",
        );
        write(&f, "posts/b.md", "+++\ntitle = \"乙\"\n+++\n正文\n");

        let out = edit_tags(
            &f.paths,
            &["posts/a.md".into(), "posts/b.md".into()],
            &TagEdit {
                add: vec!["运营".into(), " 模板 ".into()],
                remove: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(out.changed.len(), 2, "{out:?}");

        let a = read(&f, "posts/a.md");
        assert!(
            a.contains("tags = [\"模板\", \"运营\"]"),
            "原有标签保留：{a}"
        );
        assert!(a.contains("正文"), "正文不能被动：{a}");
        // 乙原本没有标签，两个都加上；前后空白被清掉
        assert!(read(&f, "posts/b.md").contains("tags = [\"运营\", \"模板\"]"));
    }

    #[test]
    fn removing_tags_leaves_the_rest_alone_and_skips_no_ops() {
        let f = fixture();
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\ntags = [\"模板\", \"运营\"]\n+++\n",
        );
        write(
            &f,
            "posts/b.md",
            "+++\ntitle = \"乙\"\ntags = [\"运营\"]\n+++\n",
        );

        let out = edit_tags(
            &f.paths,
            &["posts/a.md".into(), "posts/b.md".into()],
            &TagEdit {
                add: Vec::new(),
                remove: vec!["模板".into()],
            },
        )
        .unwrap();
        assert_eq!(out.changed, vec!["posts/a.md".to_string()]);
        assert_eq!(out.skipped.len(), 1, "乙没有这个标签，应跳过而不是空写");
        assert!(read(&f, "posts/a.md").contains("tags = [\"运营\"]"));
    }

    #[test]
    fn draft_flag_flips_only_where_needed() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\ndraft = true\n+++\n");
        write(&f, "posts/b.md", "+++\ntitle = \"乙\"\n+++\n");

        // 发布：草稿的那篇要改，已发布的跳过
        let out = set_draft(&f.paths, &["posts/a.md".into(), "posts/b.md".into()], false).unwrap();
        assert_eq!(out.changed, vec!["posts/a.md".to_string()]);
        assert_eq!(out.skipped.len(), 1);
        assert!(!read(&f, "posts/a.md").contains("draft"), "false 时删键");
    }

    #[test]
    fn moving_keeps_old_urls_and_refuses_collisions() {
        let f = fixture();
        write(&f, "posts/hello.md", "+++\ntitle = \"你好\"\n+++\n正文\n");
        write(&f, "notes/hello.md", "+++\ntitle = \"同名\"\n+++\n");
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");

        let out = move_to_section(
            &f.paths,
            &[
                "posts/hello.md".into(),
                "posts/index.md".into(),
                "posts/missing.md".into(),
            ],
            "notes",
            true,
        )
        .unwrap();

        // 同名文件已存在 + 索引页不能搬 + 找不到的文章，各跳过一条
        assert_eq!(out.moved.len(), 0, "{out:?}");
        assert_eq!(out.skipped.len(), 3, "{out:?}");
        assert!(out
            .skipped
            .iter()
            .any(|s| s.reason.contains("已存在同名文件")));
        assert!(out
            .skipped
            .iter()
            .any(|s| s.reason.contains("索引页不能搬动")));
        assert!(out.skipped.iter().any(|s| s.reason.contains("找不到")));

        // 换个不冲突的目标：搬走并补旧地址
        let out = move_to_section(&f.paths, &["posts/hello.md".into()], "essays", true).unwrap();
        assert_eq!(out.moved.len(), 1, "{out:?}");
        assert!(out.moved[0].alias_added);
        let moved = read(&f, "essays/hello.md");
        assert!(moved.contains("aliases = [\"/posts/hello/\"]"), "{moved}");
        assert!(moved.contains("正文"));
        assert!(!content::resolve_source(&f.paths.content, "posts/hello.md")
            .unwrap()
            .exists());
    }

    #[test]
    fn moving_into_the_same_section_is_a_skip() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");

        let out = move_to_section(&f.paths, &["posts/a.md".into()], "posts", true).unwrap();
        assert!(out.moved.is_empty());
        assert_eq!(out.skipped[0].reason, "已经在这个栏目里");
    }

    #[test]
    fn delete_reports_each_file_and_rejects_traversal() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");

        let out = delete(
            &f.paths,
            &[
                "posts/a.md".into(),
                "posts/gone.md".into(),
                "../../etc/passwd".into(),
            ],
        )
        .unwrap();
        assert_eq!(out.changed, vec!["posts/a.md".to_string()]);
        assert_eq!(out.skipped.len(), 2, "{out:?}");
        assert!(out
            .skipped
            .iter()
            .any(|s| s.reason.contains("越出内容目录")));
    }

    #[test]
    fn one_bad_file_does_not_abort_the_batch() {
        let f = fixture();
        write(&f, "posts/ok.md", "+++\ntitle = \"好的\"\n+++\n");
        // front matter 坏了：这一篇要报原因，另一篇照常改
        write(&f, "posts/broken.md", "+++\ntitle = 「不是 TOML」\n+++\n");

        let out = set_draft(
            &f.paths,
            &["posts/broken.md".into(), "posts/ok.md".into()],
            true,
        )
        .unwrap();
        assert_eq!(out.changed, vec!["posts/ok.md".to_string()]);
        assert_eq!(out.skipped.len(), 1);
        assert!(
            out.skipped[0].reason.contains("front matter"),
            "{:?}",
            out.skipped
        );
    }
}
