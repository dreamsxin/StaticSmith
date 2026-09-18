//! 栏目（section）管理。
//!
//! 栏目在 StaticSmith 里不是数据库里的一行，而是 `content/` 下的一层目录——
//! 这让「站点结构」始终等于「文件结构」，用编辑器、Git、资源管理器看到的都是同一件事。
//! 代价是：目录级的动作（建栏目、改栏目名、删空栏目）此前只能去文件管理器里做，
//! 而改名之后老链接会全部 404，没人会记得手工补重定向。
//!
//! 这个模块把这三件事收进程序里，并把「改名」和 `aliases` 绑在一起：
//! 移动目录的同时给每篇文章补上旧地址，构建后老链接仍然可用。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::config::{ProjectPaths, SourceFormat};
use crate::content::{self, Page};
use crate::error::{Error, Result};
use crate::frontmatter;
use crate::refs::{self, UrlMove};
use crate::util;

/// 一个栏目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    /// 相对 `content/` 的目录，根目录是空串。
    pub path: String,
    /// 展示名：有索引页就用它的标题，否则用目录名。
    pub title: String,
    /// 栏目简介，取索引页的 `description`。没有索引页时是空串。
    pub description: String,
    /// 排序权重，取索引页的 `weight`。小的在前，缺省 0。
    pub weight: i64,
    /// 栏目地址，如 `/posts/`。
    pub url: String,
    /// 栏目索引页的源文件（`index.md` / `_index.md`），没有则为 `None`。
    ///
    /// 没有索引页的栏目不会生成列表页——文章能访问，栏目本身却是 404，
    /// 界面据此提示补一个。
    pub index_source: Option<String>,
    /// 直属文章数（不含索引页，也不含子栏目里的文章）。
    pub pages: usize,
    /// 直属文章里的草稿数。
    pub drafts: usize,
    /// 直接子栏目的路径。
    pub children: Vec<String>,
}

/// 栏目元信息（索引页 front matter 里与「这个栏目是什么」有关的那几项）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Meta {
    pub title: String,
    pub description: String,
    /// 0 表示不写这个键（与「没排过序」等价）。
    pub weight: i64,
}

/// 新建结果。
#[derive(Debug, Clone, Serialize)]
pub struct Created {
    pub path: String,
    /// 顺带创建的索引页源文件。
    pub index_source: String,
}

/// 改名结果。
#[derive(Debug, Clone, Serialize)]
pub struct Renamed {
    pub from: String,
    pub to: String,
    /// 移动的文件数。
    pub moved: usize,
    /// 补了旧地址的文章数。
    pub aliases_added: usize,
    /// 站内引用被改写的篇目与条数：整栏目换名会改一批地址，指向它们的链接跟着改。
    pub refs_updated: Vec<refs::RefUpdate>,
    /// 引用没能改写的那几篇及原因（多半是 front matter 手改坏了）。
    ///
    /// 不能静默丢掉：目录已经搬走了，这几篇的链接却还指着旧地址，
    /// 不说一声用户就要等死链体检才发现。
    pub refs_failed: Vec<refs::Failure>,
}

/// 列出全部栏目，按权重、再按路径排序。根目录也算一个栏目（`path` 为空串）。
///
/// 中间层目录即使自己没有文章也会出现：`posts/2026/a.md` 会让 `posts` 与 `posts/2026`
/// 都成为栏目，否则界面里会凭空缺一层。
pub fn list(pages: &[Page]) -> Vec<Section> {
    let mut known: BTreeMap<String, Section> = BTreeMap::new();
    let ensure = |path: &str, known: &mut BTreeMap<String, Section>| {
        known.entry(path.to_string()).or_insert_with(|| Section {
            path: path.to_string(),
            title: default_title(path),
            description: String::new(),
            weight: 0,
            url: url_for(path),
            index_source: None,
            pages: 0,
            drafts: 0,
            children: Vec::new(),
        });
    };

    ensure("", &mut known);
    for page in pages {
        for ancestor in ancestors(&page.section) {
            ensure(&ancestor, &mut known);
        }
    }

    for page in pages {
        let Some(section) = known.get_mut(&page.section) else {
            continue;
        };
        if page.is_index {
            section.index_source = Some(page.source.clone());
            if !page.title.trim().is_empty() {
                section.title = page.title.clone();
            }
            section.description = page.description.clone();
            section.weight = page.weight;
            continue;
        }
        section.pages += 1;
        if page.draft {
            section.drafts += 1;
        }
    }

    let paths: Vec<String> = known.keys().cloned().collect();
    for path in &paths {
        if let Some(parent) = parent_of(path) {
            if let Some(section) = known.get_mut(&parent) {
                section.children.push(path.clone());
            }
        }
    }

    // 权重排序在最后统一做：`known` 是按路径排好的 BTreeMap，稳定排序之后
    // 「没排过序的栏目」保持路径顺序，而不是变成随机顺序。
    let order: BTreeMap<String, i64> = known
        .values()
        .map(|section| (section.path.clone(), section.weight))
        .collect();
    let mut sections: Vec<Section> = known.into_values().collect();
    for section in &mut sections {
        section
            .children
            .sort_by_key(|child| (order.get(child).copied().unwrap_or(0), child.clone()));
    }
    sections.sort_by_key(|section| section.weight);
    sections
}

/// 读写栏目元信息（标题、简介、排序权重）落在索引页的 front matter 上。
///
/// 没有单独的「栏目配置文件」是有意的：栏目就是目录，它的元信息就该在那张列表页里，
/// 否则同一件事会有两处真相。缺索引页的栏目会顺手补一张——不然改了也没处存。
///
/// 返回被改动的索引页源文件（相对 `content/`），调用方据此刷新界面与增量构建。
pub fn set_meta(paths: &ProjectPaths, path: &str, meta: &Meta) -> Result<String> {
    let relative = util::sanitize_relative_dir(path);
    if meta.title.trim().is_empty() {
        return Err(Error::Other("栏目标题不能为空".to_string()));
    }

    // 缺索引页就顺手补一张（与排栏目顺序共用 `ensure_index`：新建出来的索引页
    // 长什么样只能有一个答案）。描述随后由 patch 写进去。
    let (file, source, _) = ensure_index(paths, &relative, meta.title.trim())?;

    let raw = std::fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
    let updated = frontmatter::apply(
        &raw,
        &frontmatter::Patch {
            title: Some(meta.title.trim().to_string()),
            description: Some(meta.description.trim().to_string()),
            weight: Some(meta.weight),
            ..frontmatter::Patch::default()
        },
    )?;
    std::fs::write(&file, updated).map_err(|e| Error::io(&file, e))?;
    Ok(source)
}

/// 一栏文章重新排序的结果。
#[derive(Debug, Clone, Serialize)]
pub struct Reordered {
    /// 真正改了 front matter 的源文件，按新顺序。没动的不在里面。
    pub changed: Vec<String>,
    /// 这一栏一共几篇（也就是 weight 写到几）。
    pub total: usize,
}

/// 把一栏文章的阅读顺序固化成 `weight = 1..N`。
///
/// **为什么必须整栏一起写**：新站点里每篇的 `weight` 都是 0（「没排过」），
/// 靠日期倒序决定先后。此时「把这篇往上挪一位」根本无从表达——交换两个 0 什么也没变。
/// 所以第一次排序把整栏的当前顺序原样写成 1、2、3…，之后的每次挪动才有意义。
///
/// **为什么只写真正动了的那几篇**：每次都重写整栏的话，一次上移会让 300 篇文章
/// 全部变成「改过」——增量生成失去意义，版本库里也看不出到底动了什么。
///
/// **为什么清单必须是这一栏的全部文章**：只固化一半，剩下那些会以 weight 0
/// 跳到最前面。用户明明只想动一篇，看到的却是整栏乱了。
///
/// 读不出来的文件（front matter 手改坏了）不在这一栏的清单里——它们本来也不出现在
/// 网站上与界面里。修好之后它会带着 weight 0 回到最前面，这时再排一次即可。
pub fn reorder(paths: &ProjectPaths, section: &str, ordered: &[String]) -> Result<Reordered> {
    let relative = util::sanitize_relative_dir(section);
    let loaded = content::load_all_lenient(&paths.content, SourceFormat::default())?;

    let mut current: Vec<&Page> = loaded
        .pages
        .iter()
        .filter(|page| !page.is_index && page.section == relative)
        .collect();
    current.sort_by(|a, b| content::reading_order(a, b));

    // 先校验整份清单，一个字节都不写：写一半再报错，用户就得自己猜哪些已经生效了。
    let mut seen: Vec<&String> = Vec::new();
    for source in ordered {
        if seen.contains(&source) {
            return Err(Error::Other(format!("排序清单里有重复的文章：{source}")));
        }
        seen.push(source);
        match loaded.pages.iter().find(|page| &page.source == source) {
            Some(page) if page.is_index => {
                return Err(Error::Other(format!(
                    "{source} 是栏目索引页，它是这一栏的容器，不参与栏目内排序"
                )));
            }
            Some(page) if page.section != relative => {
                return Err(Error::Other(format!(
                    "{source} 不属于栏目 {}，跨栏目要用「移动到栏目」",
                    display_path(&relative)
                )));
            }
            Some(_) => {}
            None => {
                return Err(Error::Other(format!(
                    "{source} 不属于栏目 {}（它不存在，或者 front matter 读不出来）",
                    display_path(&relative)
                )));
            }
        }
    }
    if ordered.len() != current.len() {
        return Err(Error::Other(format!(
            "排序清单少了 {} 篇：这一栏有 {} 篇，清单里只有 {} 篇。\
             排序要给出整栏的顺序，只排一部分会让剩下的跳到最前面",
            current.len().saturating_sub(ordered.len()),
            current.len(),
            ordered.len()
        )));
    }

    let weight_of: BTreeMap<&str, i64> = current
        .iter()
        .map(|page| (page.source.as_str(), page.weight))
        .collect();

    let mut changed = Vec::new();
    for (index, source) in ordered.iter().enumerate() {
        // 1 开头而不是 0：0 是「没排过」的意思，占了它就分不出「排在最前」与「没排过」。
        let weight = index as i64 + 1;
        if weight_of.get(source.as_str()) == Some(&weight) {
            continue;
        }
        let file = content::resolve_source(&paths.content, source)?;
        let raw = std::fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
        let updated = frontmatter::apply(
            &raw,
            &frontmatter::Patch {
                weight: Some(weight),
                ..frontmatter::Patch::default()
            },
        )?;
        std::fs::write(&file, updated).map_err(|e| Error::io(&file, e))?;
        changed.push(source.clone());
    }

    Ok(Reordered {
        changed,
        total: ordered.len(),
    })
}

/// 同一层栏目重新排序的结果。
#[derive(Debug, Clone, Serialize)]
pub struct SectionsReordered {
    /// 改了位次的栏目路径，按新顺序。没动的不在里面。
    pub changed: Vec<String>,
    /// 为了存位次而顺手补出来的索引页（相对 `content/`）。
    pub created: Vec<String>,
    /// 这一层一共几个栏目。
    pub total: usize,
}

/// 把同一层栏目的顺序固化成各自索引页的 `weight = 1..N`。
///
/// 与 [`reorder`]（栏目**内**文章的位次）是同一件事的两半：一本书的目录既要能排
/// 章节之间的先后，也要能排章内小节的先后。取舍也一样——必须给出这一层的全部栏目
/// （只固化一半，剩下的会以 weight 0 跳到最前面），只写真正动了的那几个。
///
/// **缺索引页的栏目会顺手补一张。** 位次存在索引页的 front matter 里，没有索引页就无处可存，
/// 排完之后它还在原地，用户看到的是「我明明挪了它」。这与 [`set_meta`] 是同一个取舍，
/// 补出来的文件在 `created` 里报出去——悄悄新建文件是不能接受的。
///
/// `parent` 是这一层的父栏目（顶层栏目传空串）。根目录自己不参与排序：它没有同级。
pub fn reorder_sections(
    paths: &ProjectPaths,
    parent: &str,
    ordered: &[String],
) -> Result<SectionsReordered> {
    let parent_rel = util::sanitize_relative_dir(parent);
    let loaded = content::load_all_lenient(&paths.content, SourceFormat::default())?;
    let all = list(&loaded.pages);

    // `list` 已经按 weight、再按路径排好，所以这就是这一层现在的顺序。
    let current: Vec<&Section> = all
        .iter()
        .filter(|section| parent_of(&section.path).as_deref() == Some(parent_rel.as_str()))
        .collect();

    // 先校验整份清单，一个字节都不写：写一半再报错，用户就得自己猜哪几个已经生效了。
    let mut seen: Vec<&String> = Vec::new();
    for path in ordered {
        if path.is_empty() {
            return Err(Error::Other("根目录不参与排序：它没有同级栏目".to_string()));
        }
        if seen.contains(&path) {
            return Err(Error::Other(format!("排序清单里有重复的栏目：{path}")));
        }
        seen.push(path);
        if !current.iter().any(|section| &section.path == path) {
            return Err(Error::Other(format!(
                "{path} 不在 {} 这一层里，跨层要用「栏目改名」把它搬过去",
                display_path(&parent_rel)
            )));
        }
    }
    if ordered.len() != current.len() {
        return Err(Error::Other(format!(
            "排序清单少了 {} 个：这一层有 {} 个栏目，清单里只有 {} 个。\
             排序要给出整层的顺序，只排一部分会让剩下的跳到最前面",
            current.len().saturating_sub(ordered.len()),
            current.len(),
            ordered.len()
        )));
    }

    let mut out = SectionsReordered {
        changed: Vec::new(),
        created: Vec::new(),
        total: ordered.len(),
    };
    for (index, path) in ordered.iter().enumerate() {
        // 1 开头而不是 0：0 是「没排过」的意思，占了它就分不出「排在最前」与「没排过」。
        let weight = index as i64 + 1;
        let section = current
            .iter()
            .find(|section| &section.path == path)
            .expect("上面已经校验过每一项都在这一层里");
        if section.weight == weight && section.index_source.is_some() {
            continue;
        }
        let (file, source, created) = ensure_index(paths, path, &section.title)?;
        if created {
            out.created.push(source);
        }
        let raw = std::fs::read_to_string(&file).map_err(|e| Error::io(&file, e))?;
        let updated = frontmatter::apply(
            &raw,
            &frontmatter::Patch {
                weight: Some(weight),
                ..frontmatter::Patch::default()
            },
        )?;
        std::fs::write(&file, updated).map_err(|e| Error::io(&file, e))?;
        out.changed.push(path.clone());
    }
    Ok(out)
}

/// 拿到这个栏目的索引页，没有就建一张。返回 `(文件, 源路径, 是不是新建的)`。
///
/// 改栏目信息与排栏目顺序都要往索引页里写，两处各建一遍的话，「新建出来的索引页
/// 长什么样」会有两种答案。
fn ensure_index(
    paths: &ProjectPaths,
    relative: &str,
    title: &str,
) -> Result<(PathBuf, String, bool)> {
    let dir = resolve_dir(paths, relative)?;
    if !dir.is_dir() {
        return Err(Error::Other(format!("栏目不存在: {relative}")));
    }
    if let Some((file, source)) = existing_index(&dir, relative) {
        return Ok((file, source, false));
    }
    let title = if title.trim().is_empty() {
        default_title(relative)
    } else {
        title.trim().to_string()
    };
    let file = dir.join("index.md");
    std::fs::write(&file, index_skeleton(&title, "")).map_err(|e| Error::io(&file, e))?;
    Ok((file, join_source(relative, "index.md"), true))
}

/// 栏目路径给人看的写法。根目录是空串，直接印出来是一片空白。
fn display_path(relative: &str) -> &str {
    if relative.is_empty() {
        "（根目录）"
    } else {
        relative
    }
}

/// 已有的索引页（`index.md` 优先，其次 `_index.md`）。
fn existing_index(dir: &Path, relative: &str) -> Option<(PathBuf, String)> {
    ["index.md", "_index.md"].into_iter().find_map(|name| {
        let file = dir.join(name);
        file.is_file().then(|| (file, join_source(relative, name)))
    })
}

fn join_source(relative: &str, name: &str) -> String {
    if relative.is_empty() {
        name.to_string()
    } else {
        format!("{relative}/{name}")
    }
}

/// 新建栏目：建目录并写一张索引页。
///
/// 没有索引页的栏目不会生成列表页，所以这里一并创建——「建了栏目却打不开」
/// 是最容易踩的坑。索引页固定叫 `index.md`，与脚手架一致。
///
/// `description` 一并落进索引页：留空的话新栏目一建出来就会被 SEO 体检
/// 记一条 `description.missing`，等于每次建栏目都先欠一笔账。
pub fn create(paths: &ProjectPaths, path: &str, title: &str, description: &str) -> Result<Created> {
    let relative = util::sanitize_relative_dir(path);
    if relative.is_empty() {
        return Err(Error::Other("栏目名不能为空".to_string()));
    }
    let dir = resolve_dir(paths, &relative)?;
    let index = dir.join("index.md");
    if index.exists() {
        return Err(Error::Other(format!("{relative} 已经有索引页了")));
    }

    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let title = if title.trim().is_empty() {
        default_title(&relative)
    } else {
        title.trim().to_string()
    };
    std::fs::write(&index, index_skeleton(&title, description.trim()))
        .map_err(|e| Error::io(&index, e))?;

    Ok(Created {
        path: relative.clone(),
        index_source: format!("{relative}/index.md"),
    })
}

/// 栏目改名的干跑结果。
#[derive(Debug, Clone, Serialize)]
pub struct RenamePreview {
    pub from: String,
    pub to: String,
    /// 会搬动的文件数（Markdown 之外的附件也算：它们跟着目录一起走）。
    pub files: usize,
    /// 会补旧地址的文章数。关掉 `keep_aliases` 时是 0；
    /// 已经写着同一个旧地址的那几篇不重复计入。
    pub aliases: usize,
    /// 会被改写站内引用的篇目与条数。
    pub refs: Vec<refs::RefUpdate>,
    /// 改不到、得人工看一眼的相对链接：哪几篇、各几处。
    ///
    /// 相对地址（`../a/`）要按引用方所在目录解析，而改名改的是被引用方，两者对不上。
    pub refs_manual: Vec<refs::RefUpdate>,
}

/// 改名前的校验与准备，预览与执行共用。
///
/// 五种拒绝理由（根目录、同名、搬进自己、栏目不存在、目标已存在）只在这里判一次：
/// 两处各写一遍，迟早出现「预览通过、执行报错」。
fn rename_setup(
    paths: &ProjectPaths,
    from: &str,
    to: &str,
) -> Result<(String, String, PathBuf, PathBuf, Vec<Page>)> {
    let from_rel = util::sanitize_relative_dir(from);
    let to_rel = util::sanitize_relative_dir(to);
    if from_rel.is_empty() || to_rel.is_empty() {
        return Err(Error::Other("栏目名不能为空（根目录不能改名）".to_string()));
    }
    if from_rel == to_rel {
        return Err(Error::Other("新旧栏目名相同".to_string()));
    }
    // 搬到自己的子目录里会把目录树套进自己，文件系统层面也未必报错
    if to_rel.starts_with(&format!("{from_rel}/")) {
        return Err(Error::Other(format!(
            "{to_rel} 在 {from_rel} 之内，无法移动"
        )));
    }

    let source_dir = resolve_dir(paths, &from_rel)?;
    let target_dir = resolve_dir(paths, &to_rel)?;
    if !source_dir.is_dir() {
        return Err(Error::Other(format!("栏目不存在: {from_rel}")));
    }
    if target_dir.exists() {
        return Err(Error::Other(format!("{to_rel} 已存在，请换个名字")));
    }

    // 移动前先记下这棵子树里的页面：移动之后旧地址就算不出来了。
    // 宽容加载（一篇坏文件不该让改名整体失败），但**子树里有读不出来的就拒绝改名**：
    // 那一篇会跟着目录一起搬走，而它的旧地址算不出来、补不上 alias——
    // 静默丢一条重定向比拒绝一次难查得多。
    let loaded = content::load_all_lenient(&paths.content, SourceFormat::default())?;
    let in_subtree =
        |section: &str| section == from_rel || section.starts_with(&format!("{from_rel}/"));
    let broken: Vec<_> = loaded
        .broken
        .iter()
        .filter(|item| {
            let dir = item
                .source
                .rsplit_once('/')
                .map(|(dir, _)| dir)
                .unwrap_or("");
            in_subtree(dir)
        })
        .cloned()
        .collect();
    if !broken.is_empty() {
        let summary = crate::skips::summarize(&broken);
        return Err(Error::Other(format!(
            "这个栏目里有文件读不出来，改名会让它的旧地址补不上，先修好：{}（{}）",
            summary.line,
            summary.details.join("；")
        )));
    }
    let pages: Vec<Page> = loaded
        .pages
        .into_iter()
        .filter(|page| in_subtree(&page.section))
        .collect();

    Ok((from_rel, to_rel, source_dir, target_dir, pages))
}

/// 这批页面的地址会怎么变。改名只换目录前缀，所以是一次前缀替换。
fn rename_url_moves(pages: &[Page], from_rel: &str, to_rel: &str) -> Vec<UrlMove> {
    pages
        .iter()
        .filter_map(|page| {
            let rest = page.url.strip_prefix(&format!("/{from_rel}"))?;
            Some(UrlMove {
                from: page.url.clone(),
                to: format!("/{to_rel}{rest}"),
            })
        })
        .collect()
}

/// 干跑一次栏目改名：会搬几个文件、给几篇补旧地址、改写哪几篇里的几处引用。不碰磁盘。
///
/// 三条「改地址」的路里，栏目改名一次动的地址最多（整棵子树），却是最后一个补上
/// 干跑的——搬动、改 slug 早就有了。
pub fn preview_rename(
    paths: &ProjectPaths,
    from: &str,
    to: &str,
    keep_aliases: bool,
) -> Result<RenamePreview> {
    let (from_rel, to_rel, source_dir, _, pages) = rename_setup(paths, from, to)?;

    let files = WalkDir::new(&source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count();

    // 与 `add_alias` 的判断保持一致：已经写着这个旧地址的那篇不会被再写一次
    let aliases = if keep_aliases {
        pages
            .iter()
            .filter(|page| {
                is_markdown(Path::new(&page.source)) && !page.aliases.contains(&page.url)
            })
            .count()
    } else {
        0
    };

    let url_moves = rename_url_moves(&pages, &from_rel, &to_rel);
    Ok(RenamePreview {
        from: from_rel,
        to: to_rel,
        files,
        aliases,
        refs: refs::preview_site(&paths.content, &url_moves)?.updated,
        refs_manual: refs::manual_review(&paths.content, &url_moves)?,
    })
}

/// 栏目改名（等于把目录搬到新名字下）。
///
/// `keep_aliases` 为真时给每篇被移动的文章补上旧地址，构建会为旧地址生成重定向页，
/// 站外的老链接与搜索结果不会因为整理结构而全部失效。默认就该开着。
pub fn rename(paths: &ProjectPaths, from: &str, to: &str, keep_aliases: bool) -> Result<Renamed> {
    let (from_rel, to_rel, source_dir, target_dir, pages) = rename_setup(paths, from, to)?;
    let old_urls: BTreeMap<String, String> = pages
        .iter()
        .map(|page| (page.source.clone(), page.url.clone()))
        .collect();

    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::rename(&source_dir, &target_dir).map_err(|e| Error::io(&source_dir, e))?;

    let mut moved = 0;
    let mut aliases_added = 0;
    for entry in WalkDir::new(&target_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        moved += 1;
        if !keep_aliases || !is_markdown(entry.path()) {
            continue;
        }
        let relative = util::to_slash(
            entry
                .path()
                .strip_prefix(&paths.content)
                .unwrap_or(entry.path()),
        );
        // 旧 source 与新 source 只差目录前缀
        let old_source = relative.replacen(&to_rel, &from_rel, 1);
        let Some(old_url) = old_urls.get(&old_source) else {
            continue;
        };
        if content::add_alias(entry.path(), old_url)? {
            aliases_added += 1;
        }
    }

    // 整栏目换名等于一批地址同时变：把站内指向它们的链接一并改到新位置。
    // 旧地址仍然照原样补进 aliases，站外的老链接靠重定向页兜着——两件事都要做。
    let rewritten = refs::rewrite_site(
        &paths.content,
        &rename_url_moves(&pages, &from_rel, &to_rel),
    )?;

    Ok(Renamed {
        from: from_rel,
        to: to_rel,
        moved,
        aliases_added,
        refs_updated: rewritten.updated,
        refs_failed: rewritten.failed,
    })
}

/// 删除空栏目。
///
/// 里面还有文件就拒绝：删目录是不可逆的，而「顺手把二十篇文章一起删了」
/// 绝不该是一次点击的后果。要清空请先逐篇删除或移走。
pub fn remove(paths: &ProjectPaths, path: &str) -> Result<()> {
    let relative = util::sanitize_relative_dir(path);
    if relative.is_empty() {
        return Err(Error::Other("根目录不能删除".to_string()));
    }
    let dir = resolve_dir(paths, &relative)?;
    if !dir.is_dir() {
        return Err(Error::Other(format!("栏目不存在: {relative}")));
    }

    let files: Vec<String> = WalkDir::new(&dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| util::to_slash(e.path().strip_prefix(&dir).unwrap_or(e.path())))
        .collect();
    // 只剩索引页时可以连它一起删：那是新建栏目时自动生成的，不算用户内容
    let leftovers: Vec<&String> = files
        .iter()
        .filter(|f| *f != "index.md" && *f != "_index.md")
        .collect();
    if !leftovers.is_empty() {
        return Err(Error::Other(format!(
            "{relative} 里还有 {} 个文件，请先移走或删除",
            leftovers.len()
        )));
    }

    std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))
}

/// 目录路径，并确认它没有越出 `content/`。
///
/// 校验放在字符串上而不是文件系统上：待建的目录还不存在，`canonicalize` 会失败，
/// 而 Windows 上「盘符前缀会替换整条路径」这种坑必须在 `join` 之前挡掉。
fn resolve_dir(paths: &ProjectPaths, relative: &str) -> Result<PathBuf> {
    let segments: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
    let escapes = segments
        .iter()
        .any(|s| *s == ".." || *s == "." || s.contains(':') || s.starts_with('\\'));
    if escapes {
        return Err(Error::Other(format!("{relative} 不是合法的栏目名")));
    }
    // Windows 的设备名建不成目录（也不该建），三个平台一起拦，理由同 `resolve_source`。
    if let Some(bad) = segments.iter().find(|s| crate::util::is_reserved_name(s)) {
        return Err(Error::Other(format!(
            "{bad} 是系统保留名（Windows 上它是设备而不是目录），请改个栏目名"
        )));
    }
    let dir = segments
        .iter()
        .fold(paths.content.clone(), |acc, s| acc.join(s));
    // 内容目录里若已有一条指向别处的符号链接，字面校验看不出来。
    if !crate::util::is_within_resolved(&paths.content, &dir) {
        return Err(Error::Other(format!(
            "{relative} 经由符号链接走出了内容目录"
        )));
    }
    Ok(dir)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn index_skeleton(title: &str, description: &str) -> String {
    format!(
        "+++\ntitle = \"{}\"\ndescription = \"{}\"\n+++\n",
        toml_escape(title),
        toml_escape(description)
    )
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn default_title(path: &str) -> String {
    match path.rsplit('/').next().filter(|s| !s.is_empty()) {
        Some(name) => name.to_string(),
        None => "根目录".to_string(),
    }
}

fn url_for(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}/")
    }
}

/// `a/b/c` → `["a", "a/b", "a/b/c"]`。空串返回空列表（根目录单独补）。
fn ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        out.push(current.clone());
    }
    out
}

fn parent_of(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    Some(match path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    })
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
        let path = source
            .split('/')
            .fold(f.paths.content.clone(), |acc, s| acc.join(s));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    fn sections(f: &Fixture) -> Vec<Section> {
        list(&content::load_all(&f.paths.content, SourceFormat::default()).unwrap())
    }

    /// 栏目改名是三条「改地址」的路里一次动得最多的（整棵子树），
    /// 却曾是唯一直接落盘的。这几条钉住它的干跑。
    #[test]
    fn rename_preview_agrees_with_the_rename_and_touches_nothing() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n\n正文\n");
        write(&f, "posts/cover.png", "not really a png");
        write(
            &f,
            "notes/b.md",
            "+++\ntitle = \"乙\"\n+++\n\n见 [甲](/posts/a/) 与 [列表](/posts/)\n",
        );

        let dry = preview_rename(&f.paths, "posts", "essays", true).unwrap();
        assert_eq!(dry.from, "posts");
        assert_eq!(dry.to, "essays");
        assert_eq!(dry.files, 3, "附件也会跟着搬");
        assert_eq!(dry.aliases, 2, "两篇 Markdown 会补旧地址，图片不算");
        assert_eq!(dry.refs.len(), 1);
        assert_eq!(dry.refs[0].source, "notes/b.md");
        assert_eq!(dry.refs[0].hits, 2, "文章与栏目列表两个地址都会改");
        // 干跑不碰磁盘
        assert!(f.paths.content.join("posts/a.md").is_file());
        assert!(!f.paths.content.join("essays").exists());

        let done = rename(&f.paths, "posts", "essays", true).unwrap();
        assert_eq!(done.moved, dry.files, "预览说搬几个就搬几个");
        assert_eq!(done.aliases_added, dry.aliases);
        assert_eq!(done.refs_updated, dry.refs);
    }

    #[test]
    fn rename_preview_refuses_exactly_what_the_rename_refuses() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");
        write(&f, "notes/b.md", "+++\ntitle = \"乙\"\n+++\n");

        for (from, to) in [
            ("posts", ""),          // 根目录不能改名
            ("posts", "posts"),     // 新旧同名
            ("posts", "posts/sub"), // 搬进自己里面
            ("missing", "x"),       // 栏目不存在
            ("posts", "notes"),     // 目标已存在
        ] {
            assert!(
                preview_rename(&f.paths, from, to, true).is_err(),
                "{from} → {to} 该被拦下"
            );
            assert!(rename(&f.paths, from, to, true).is_err());
        }
    }

    #[test]
    fn rename_preview_counts_no_aliases_when_they_are_turned_off() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");

        let dry = preview_rename(&f.paths, "posts", "essays", false).unwrap();
        assert_eq!(dry.aliases, 0);
        assert_eq!(dry.files, 1);
    }

    fn find<'a>(list: &'a [Section], path: &str) -> &'a Section {
        list.iter()
            .find(|s| s.path == path)
            .unwrap_or_else(|| panic!("找不到栏目 {path}，实际有 {list:?}"))
    }

    #[test]
    fn lists_sections_with_counts_and_index_pages() {
        let f = fixture();
        write(&f, "index.md", "+++\ntitle = \"首页\"\n+++\n");
        write(&f, "posts/index.md", "+++\ntitle = \"文章归档\"\n+++\n");
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");
        write(&f, "posts/b.md", "+++\ntitle = \"乙\"\ndraft = true\n+++\n");
        write(&f, "posts/2026/c.md", "+++\ntitle = \"丙\"\n+++\n");

        let list = sections(&f);
        let root = find(&list, "");
        assert_eq!(root.title, "首页", "根栏目用索引页标题");
        assert_eq!(root.url, "/");
        assert_eq!(root.children, vec!["posts".to_string()]);

        let posts = find(&list, "posts");
        assert_eq!(posts.title, "文章归档");
        assert_eq!(posts.url, "/posts/");
        assert_eq!(posts.index_source.as_deref(), Some("posts/index.md"));
        assert_eq!(posts.pages, 2, "索引页不算文章");
        assert_eq!(posts.drafts, 1);
        assert_eq!(posts.children, vec!["posts/2026".to_string()]);

        // 中间层目录即使没有索引页也要出现，并被标成缺索引
        let nested = find(&list, "posts/2026");
        assert_eq!(nested.title, "2026");
        assert!(nested.index_source.is_none());
        assert_eq!(nested.pages, 1);
    }

    #[test]
    fn create_makes_the_directory_and_its_index_page() {
        let f = fixture();
        let created = create(&f.paths, "notes", "随手记", "零散的记录").unwrap();
        assert_eq!(created.index_source, "notes/index.md");

        let raw = std::fs::read_to_string(f.paths.content.join("notes/index.md")).unwrap();
        assert!(raw.contains("title = \"随手记\""), "{raw}");
        // 简介当场写进索引页：不写的话新栏目一建出来就会被 SEO 体检记一条缺描述
        assert!(raw.contains("description = \"零散的记录\""), "{raw}");

        // 再建一次不覆盖
        let err = create(&f.paths, "notes", "随手记", "").unwrap_err();
        assert!(err.to_string().contains("已经有索引页"), "{err}");

        // 标题留空时退回目录名
        create(&f.paths, "logs", "  ", "").unwrap();
        let raw = std::fs::read_to_string(f.paths.content.join("logs/index.md")).unwrap();
        assert!(raw.contains("title = \"logs\""), "{raw}");
        // 简介也留空时仍然留一个空键，编辑器里一眼能看到该补什么
        assert!(raw.contains("description = \"\""), "{raw}");
    }

    #[test]
    fn rename_moves_files_and_keeps_old_urls_alive() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        write(&f, "posts/hello.md", "+++\ntitle = \"你好\"\n+++\n正文\n");
        write(&f, "posts/2026/deep.md", "+++\ntitle = \"深处\"\n+++\n");

        let report = rename(&f.paths, "posts", "blog", true).unwrap();
        assert_eq!(report.moved, 3);
        assert_eq!(report.aliases_added, 3);
        assert!(!f.paths.content.join("posts").exists());

        let hello = std::fs::read_to_string(f.paths.content.join("blog/hello.md")).unwrap();
        assert!(hello.contains("aliases = [\"/posts/hello/\"]"), "{hello}");
        assert!(hello.contains("正文"), "正文必须原样保留：{hello}");

        let index = std::fs::read_to_string(f.paths.content.join("blog/index.md")).unwrap();
        assert!(index.contains("aliases = [\"/posts/\"]"), "{index}");

        let deep = std::fs::read_to_string(f.paths.content.join("blog/2026/deep.md")).unwrap();
        assert!(deep.contains("aliases = [\"/posts/2026/deep/\"]"), "{deep}");
    }

    #[test]
    fn rename_can_skip_aliases_and_refuses_bad_targets() {
        let f = fixture();
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");
        write(&f, "notes/b.md", "+++\ntitle = \"乙\"\n+++\n");

        // 目标已存在
        let err = rename(&f.paths, "posts", "notes", true).unwrap_err();
        assert!(err.to_string().contains("已存在"), "{err}");

        // 搬进自己的子目录
        let err = rename(&f.paths, "posts", "posts/inner", true).unwrap_err();
        assert!(err.to_string().contains("无法移动"), "{err}");

        // 不留旧地址时不动 front matter
        rename(&f.paths, "posts", "essays", false).unwrap();
        let raw = std::fs::read_to_string(f.paths.content.join("essays/a.md")).unwrap();
        assert!(!raw.contains("aliases"), "{raw}");
    }

    #[test]
    fn remove_only_deletes_empty_sections() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");

        let err = remove(&f.paths, "posts").unwrap_err();
        assert!(err.to_string().contains("还有 1 个文件"), "{err}");
        assert!(f.paths.content.join("posts/a.md").exists());

        // 只剩自动生成的索引页时可以整栏删掉
        std::fs::remove_file(f.paths.content.join("posts/a.md")).unwrap();
        remove(&f.paths, "posts").unwrap();
        assert!(!f.paths.content.join("posts").exists());

        // 根目录与越界路径都拒绝
        assert!(remove(&f.paths, "").is_err());
        assert!(remove(&f.paths, "../../etc").is_err());
    }

    #[test]
    fn meta_lands_in_the_index_page_and_orders_sections() {
        let f = fixture();
        write(
            &f,
            "posts/index.md",
            "+++\ntitle = \"文章\"\n+++\n列表页正文\n",
        );
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");
        write(&f, "notes/n.md", "+++\ntitle = \"随手\"\n+++\n");

        // notes 还没有索引页，改元信息时顺手补一张
        let source = set_meta(
            &f.paths,
            "notes",
            &Meta {
                title: "随手记".into(),
                description: "短小的记录".into(),
                weight: 1,
            },
        )
        .unwrap();
        assert_eq!(source, "notes/index.md");

        set_meta(
            &f.paths,
            "posts",
            &Meta {
                title: "文章".into(),
                description: "长文".into(),
                weight: 2,
            },
        )
        .unwrap();

        let raw = std::fs::read_to_string(f.paths.content.join("posts/index.md")).unwrap();
        assert!(raw.contains("description = \"长文\""), "{raw}");
        assert!(raw.contains("weight = 2"), "{raw}");
        assert!(raw.contains("列表页正文"), "正文必须原样保留：{raw}");

        let list = sections(&f);
        assert_eq!(find(&list, "notes").description, "短小的记录");
        assert_eq!(find(&list, "notes").weight, 1);
        // 权重小的在前；没排过序的根目录（weight 0）更靠前
        let order: Vec<&str> = list.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(order, vec!["", "notes", "posts"]);
        assert_eq!(
            find(&list, "").children,
            vec!["notes".to_string(), "posts".to_string()],
            "子栏目也按权重排"
        );
    }

    #[test]
    fn meta_clears_empty_description_and_zero_weight() {
        let f = fixture();
        write(
            &f,
            "posts/index.md",
            "+++\ntitle = \"文章\"\ndescription = \"旧简介\"\nweight = 5\n+++\n",
        );

        set_meta(
            &f.paths,
            "posts",
            &Meta {
                title: "文章".into(),
                description: String::new(),
                weight: 0,
            },
        )
        .unwrap();

        // 空值等于不写：留下 description = "" 只会让源文越写越长
        let raw = std::fs::read_to_string(f.paths.content.join("posts/index.md")).unwrap();
        assert!(!raw.contains("description"), "{raw}");
        assert!(!raw.contains("weight"), "{raw}");
    }

    #[test]
    fn meta_refuses_empty_title_and_missing_sections() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");

        let err = set_meta(&f.paths, "posts", &Meta::default()).unwrap_err();
        assert!(err.to_string().contains("标题不能为空"), "{err}");

        let err = set_meta(
            &f.paths,
            "ghost",
            &Meta {
                title: "不存在".into(),
                ..Meta::default()
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("栏目不存在"), "{err}");
    }

    #[test]
    fn traversal_in_names_is_stripped_before_touching_disk() {
        let f = fixture();
        // sanitize 之后是 evil，不会跑到内容目录之外
        let created = create(&f.paths, "../../evil", "越界", "").unwrap();
        assert_eq!(created.path, "evil");
        assert!(f.paths.content.join("evil/index.md").is_file());
    }

    /// 这一栏最终的阅读顺序。用 `content::reading_order` 算，也就是网站上的顺序。
    fn order_of(f: &Fixture, section: &str) -> Vec<String> {
        let pages = content::load_all(&f.paths.content, SourceFormat::default()).unwrap();
        let mut items: Vec<&content::Page> = pages
            .iter()
            .filter(|p| !p.is_index && p.section == section)
            .collect();
        items.sort_by(|a, b| content::reading_order(a, b));
        items.iter().map(|p| p.source.clone()).collect()
    }

    /// 「上移一篇」在 weight 全是 0 的站点里无从表达——三篇都是 0，交换两个 0 什么也没变。
    /// 所以第一次排序必须把整栏的顺序**固化**成 1..N。
    #[test]
    fn reorder_freezes_the_whole_section_into_weights() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        // 日期倒序时天然是 c、b、a；现在要的是 b、a、c
        write(
            &f,
            "posts/a.md",
            "+++\ntitle = \"甲\"\ndate = \"2026-01-01\"\n+++\n",
        );
        write(
            &f,
            "posts/b.md",
            "+++\ntitle = \"乙\"\ndate = \"2026-01-02\"\n+++\n",
        );
        write(
            &f,
            "posts/c.md",
            "+++\ntitle = \"丙\"\ndate = \"2026-01-03\"\n+++\n",
        );
        assert_eq!(
            order_of(&f, "posts"),
            ["posts/c.md", "posts/b.md", "posts/a.md"],
            "没排过的按日期倒序"
        );

        let wanted = vec![
            "posts/b.md".to_string(),
            "posts/a.md".to_string(),
            "posts/c.md".to_string(),
        ];
        let done = reorder(&f.paths, "posts", &wanted).unwrap();

        assert_eq!(done.changed, wanted, "三篇都没排过，三篇都要写");
        assert_eq!(done.total, 3);
        assert_eq!(order_of(&f, "posts"), wanted, "网站上的顺序就得是这个顺序");
        // 1 开头而不是 0：0 是「没排过」，占了它就分不出「排在最前」与「没排过」
        let raw = std::fs::read_to_string(f.paths.content.join("posts/b.md")).unwrap();
        assert!(raw.contains("weight = 1"), "{raw}");
    }

    /// 固化过之后，「上移一篇」只该动那两篇。
    ///
    /// 每次都重写整栏的话，一次上移会让 300 篇文章全部进入「改过」状态：
    /// 增量生成失去意义，版本库里也看不出到底动了什么。
    #[test]
    fn reorder_only_writes_the_pages_that_actually_move() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        for (name, weight) in [("a", 1), ("b", 2), ("c", 3)] {
            write(
                &f,
                &format!("posts/{name}.md"),
                &format!("+++\ntitle = \"{name}\"\nweight = {weight}\n+++\n"),
            );
        }

        let done = reorder(
            &f.paths,
            "posts",
            &[
                "posts/b.md".to_string(),
                "posts/a.md".to_string(),
                "posts/c.md".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(done.changed, ["posts/b.md", "posts/a.md"], "c 没动就别碰它");
        assert_eq!(
            order_of(&f, "posts"),
            ["posts/b.md", "posts/a.md", "posts/c.md"]
        );
    }

    /// 只给半栏是不行的。
    ///
    /// 固化一半会让剩下的那些以 weight 0 排到**最前面**——用户明明只想动一篇，
    /// 看到的却是整栏乱了。所以清单必须是这一栏的全部文章，不多不少。
    #[test]
    fn reorder_refuses_anything_but_the_whole_section() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        write(&f, "posts/a.md", "+++\ntitle = \"甲\"\n+++\n");
        write(&f, "posts/b.md", "+++\ntitle = \"乙\"\n+++\n");
        write(&f, "notes/c.md", "+++\ntitle = \"丙\"\n+++\n");

        for (list, expect) in [
            (vec!["posts/a.md"], "少了"),
            (vec!["posts/a.md", "posts/b.md", "notes/c.md"], "不属于"),
            (vec!["posts/a.md", "posts/b.md", "posts/index.md"], "索引页"),
            (vec!["posts/a.md", "posts/a.md"], "重复"),
        ] {
            let sources: Vec<String> = list.iter().map(|s| s.to_string()).collect();
            let err = reorder(&f.paths, "posts", &sources)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(expect),
                "{sources:?} 该说「{expect}」，得到：{err}"
            );
        }

        // 一次都没写成，磁盘上还是原样
        let raw = std::fs::read_to_string(f.paths.content.join("posts/a.md")).unwrap();
        assert!(!raw.contains("weight"), "{raw}");
    }

    /// 这一层的栏目路径，按站点上列出的顺序。
    fn level(f: &Fixture, parent: &str) -> Vec<String> {
        sections(f)
            .into_iter()
            .filter(|s| !s.path.is_empty() && parent_of(&s.path).as_deref() == Some(parent))
            .map(|s| s.path)
            .collect()
    }

    /// 栏目的位次也要能挪，理由与文章那条路一样：全是 weight 0 时「上移一位」无从表达。
    ///
    /// 界面上原先只有一个「排序」数字输入框——那是把实现细节摆给用户看。
    #[test]
    fn reorder_sections_freezes_the_sibling_order_into_index_pages() {
        let f = fixture();
        for name in ["guides", "notes", "posts"] {
            write(
                &f,
                &format!("{name}/index.md"),
                &format!("+++\ntitle = \"{name}\"\n+++\n"),
            );
        }
        // 都没排过（weight 0），所以现在按路径排
        assert_eq!(level(&f, ""), ["guides", "notes", "posts"]);

        let wanted = vec![
            "posts".to_string(),
            "notes".to_string(),
            "guides".to_string(),
        ];
        let done = reorder_sections(&f.paths, "", &wanted).unwrap();

        assert_eq!(done.changed, wanted, "三个都没排过，三个都要写");
        assert!(done.created.is_empty(), "索引页本来就有");
        assert_eq!(done.total, 3);
        assert_eq!(level(&f, ""), wanted, "站点上的栏目顺序就得是这个顺序");
        let raw = std::fs::read_to_string(f.paths.content.join("posts/index.md")).unwrap();
        assert!(raw.contains("weight = 1"), "{raw}");
    }

    /// 位次存在索引页的 front matter 里，所以缺索引页的栏目得顺手补一张。
    ///
    /// 不补的话它的位次无处可存，排完之后它还在原地——用户看到的是「我明明挪了它」。
    /// 这与改栏目信息（`set_meta`）是同一个取舍，补出来的文件必须报给调用方。
    #[test]
    fn reorder_sections_creates_a_missing_index_page_to_store_the_position() {
        let f = fixture();
        write(&f, "posts/index.md", "+++\ntitle = \"文章\"\n+++\n");
        // 只有文章、没有索引页的栏目
        write(&f, "notes/a.md", "+++\ntitle = \"甲\"\n+++\n");

        let done =
            reorder_sections(&f.paths, "", &["notes".to_string(), "posts".to_string()]).unwrap();

        assert_eq!(done.created, ["notes/index.md"]);
        let raw = std::fs::read_to_string(f.paths.content.join("notes/index.md")).unwrap();
        // 标题用目录名兜底，别写出一张没标题的列表页
        assert!(raw.contains("title = \"notes\""), "{raw}");
        assert!(raw.contains("weight = 1"), "{raw}");
        assert_eq!(level(&f, ""), ["notes", "posts"]);
    }

    /// 只给同一层的一部分是不行的，理由同文章排序：剩下的会以 weight 0 跳到最前面。
    #[test]
    fn reorder_sections_refuses_anything_but_the_whole_level() {
        let f = fixture();
        for name in ["posts", "notes"] {
            write(
                &f,
                &format!("{name}/index.md"),
                &format!("+++\ntitle = \"{name}\"\n+++\n"),
            );
        }
        write(&f, "posts/2026/index.md", "+++\ntitle = \"2026\"\n+++\n");

        for (paths, expect) in [
            (vec!["posts"], "少了"),
            (vec!["posts", "notes", "posts/2026"], "不在"),
            (vec!["posts", "posts"], "重复"),
            (vec!["posts", ""], "根目录"),
        ] {
            let ordered: Vec<String> = paths.iter().map(|s| s.to_string()).collect();
            let err = reorder_sections(&f.paths, "", &ordered)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(expect),
                "{ordered:?} 该说「{expect}」，得到：{err}"
            );
        }

        // 一次都没写成
        let raw = std::fs::read_to_string(f.paths.content.join("posts/index.md")).unwrap();
        assert!(!raw.contains("weight"), "{raw}");
    }
}
