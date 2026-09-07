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

use crate::config::ProjectPaths;
use crate::content::{self, Page};
use crate::error::{Error, Result};
use crate::frontmatter::{self, Patch};
use crate::util;

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
    let add: Vec<&str> = edit
        .add
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let remove: Vec<&str> = edit
        .remove
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if add.is_empty() && remove.is_empty() {
        return Err(Error::Other("没有要增删的标签".to_string()));
    }

    each(paths, sources, |raw| {
        let mut tags = frontmatter::read(raw)?.tags;
        let before = tags.clone();
        tags.retain(|t| !remove.iter().any(|r| r == t));
        for tag in &add {
            if !tags.iter().any(|t| t == tag) {
                tags.push((*tag).to_string());
            }
        }
        if tags == before {
            return Ok(None);
        }
        Ok(Some(Patch {
            tags: Some(tags),
            ..Patch::default()
        }))
    })
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
    let pages = content::load_all(&paths.content)?;
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

/// 搬一篇。返回 `None` 表示本来就在目标栏目。
fn move_one(
    paths: &ProjectPaths,
    page: &Page,
    target: &str,
    keep_aliases: bool,
) -> Result<Option<Moved>> {
    if page.section == target {
        return Ok(None);
    }
    let file_name = page
        .source
        .rsplit('/')
        .next()
        .ok_or_else(|| Error::Other(format!("源路径异常: {}", page.source)))?;
    // 栏目索引页搬走会让原栏目失去列表页，同时在新栏目里撞上人家的索引页
    if page.is_index {
        return Err(Error::Other(
            "栏目索引页不能搬动，请改栏目名或新建栏目".to_string(),
        ));
    }

    let new_source = if target.is_empty() {
        file_name.to_string()
    } else {
        format!("{target}/{file_name}")
    };
    let from_path = content::resolve_source(&paths.content, &page.source);
    let to_path = content::resolve_source(&paths.content, &new_source);
    if to_path.exists() {
        return Err(Error::Other(format!("{new_source} 已存在同名文件")));
    }
    if let Some(parent) = to_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::rename(&from_path, &to_path).map_err(|e| Error::io(&from_path, e))?;

    let mut alias_added = false;
    if keep_aliases {
        alias_added = add_alias(&to_path, &page.url)?;
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
        let path = content::resolve_source(&paths.content, source);
        if !under_content(paths, &path) {
            out.skipped.push(Skipped {
                source: source.clone(),
                reason: "不在内容目录内".to_string(),
            });
            continue;
        }
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

/// 逐篇读源文、算出补丁、写回。补丁为 `None` 表示这篇无需改动。
fn each(
    paths: &ProjectPaths,
    sources: &[String],
    mut patch_for: impl FnMut(&str) -> Result<Option<Patch>>,
) -> Result<Outcome> {
    let mut out = Outcome::default();
    for source in sources {
        let path = content::resolve_source(&paths.content, source);
        if !under_content(paths, &path) {
            out.skipped.push(Skipped {
                source: source.clone(),
                reason: "不在内容目录内".to_string(),
            });
            continue;
        }
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

/// 传进来的 source 可能来自 Agent，越界的一律拒绝。
fn under_content(paths: &ProjectPaths, path: &Path) -> bool {
    let segments = path.strip_prefix(&paths.content).ok();
    match segments {
        Some(rest) => !rest
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        None => false,
    }
}

fn add_alias(file: &Path, old_url: &str) -> Result<bool> {
    let raw = std::fs::read_to_string(file).map_err(|e| Error::io(file, e))?;
    match frontmatter::push_alias(&raw, old_url)? {
        Some(updated) => {
            std::fs::write(file, updated).map_err(|e| Error::io(file, e))?;
            Ok(true)
        }
        None => Ok(false),
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
        assert!(!content::resolve_source(&f.paths.content, "posts/hello.md").exists());
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
            .any(|s| s.reason.contains("不在内容目录内")));
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
