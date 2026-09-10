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
    let dir = resolve_dir(paths, &relative)?;
    if !dir.is_dir() {
        return Err(Error::Other(format!("栏目不存在: {relative}")));
    }
    if meta.title.trim().is_empty() {
        return Err(Error::Other("栏目标题不能为空".to_string()));
    }

    let (file, source) = match existing_index(&dir, &relative) {
        Some(found) => found,
        None => {
            let file = dir.join("index.md");
            std::fs::write(
                &file,
                index_skeleton(meta.title.trim(), meta.description.trim()),
            )
            .map_err(|e| Error::io(&file, e))?;
            let source = join_source(&relative, "index.md");
            (file, source)
        }
    };

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

/// 栏目改名（等于把目录搬到新名字下）。
///
/// `keep_aliases` 为真时给每篇被移动的文章补上旧地址，构建会为旧地址生成重定向页，
/// 站外的老链接与搜索结果不会因为整理结构而全部失效。默认就该开着。
pub fn rename(paths: &ProjectPaths, from: &str, to: &str, keep_aliases: bool) -> Result<Renamed> {
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

    // 移动前先记下每篇文章的旧地址：移动之后就算不出来了
    let old_urls: BTreeMap<String, String> =
        content::load_all(&paths.content, SourceFormat::default())?
            .into_iter()
            .filter(|page| {
                page.section == from_rel || page.section.starts_with(&format!("{from_rel}/"))
            })
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

    Ok(Renamed {
        from: from_rel,
        to: to_rel,
        moved,
        aliases_added,
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
}
