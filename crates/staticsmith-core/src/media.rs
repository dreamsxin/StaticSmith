//! 媒体资源体检：找出没人引用的文件，以及引用了却不存在的文件。
//!
//! 内容寻址存储只保证「同一张图只存一份」，不管「这张图还有没有人用」。
//! 删掉一篇文章后它引用过的图片会一直留在磁盘上，站点越运营越胖；
//! 反过来，改错路径或手动删了文件，页面上就是一个破图标——两者都要能查出来。
//!
//! 引用范围包含内容、模板与主题：`logo.png` 常常只被 `components/header.html`
//! 或主题 CSS 的 `background-image` 引用，只扫内容会把它误判成垃圾并删掉。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use walkdir::WalkDir;

use crate::config::{Assets as AssetsConfig, ProjectPaths};
use crate::error::{Error, Result};
use crate::util;

/// 会被扫描引用的文本类型。二进制文件没必要读。
const TEXT_EXTENSIONS: &[&str] = &[
    "md", "markdown", "html", "htm", "css", "js", "json", "xml", "txt", "toml",
];

/// 一个媒体文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaFile {
    /// 相对 `static_dir` 的路径，正斜杠
    pub path: String,
    /// 站内引用地址
    pub url: String,
    pub size: u64,
}

/// 被引用但磁盘上不存在的地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingRef {
    pub url: String,
    /// 引用它的文件（相对站点根，正斜杠），便于直接去改
    pub referenced_by: Vec<String>,
}

/// 体检报告。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// 资源目录里的文件总数
    pub total: usize,
    pub total_size: u64,
    /// 没有任何内容、模板或主题文件引用的文件
    pub unused: Vec<MediaFile>,
    /// 删掉 `unused` 能回收的字节数
    pub reclaimable: u64,
    pub missing: Vec<MissingRef>,
}

/// 删除结果。
#[derive(Debug, Clone, Serialize)]
pub struct Removed {
    pub removed: Vec<String>,
    pub freed: u64,
}

/// 体检。不写盘。
pub fn audit(paths: &ProjectPaths, config: &AssetsConfig) -> Result<Report> {
    let files = collect_media(paths, config)?;
    let sources = collect_sources(paths);

    // 用文件名而不是完整 URL 判定引用：url_prefix 可能指向 CDN，
    // 而哈希命名的文件名本身就足够独特，改前缀不该让体检失效。
    let mut unused = Vec::new();
    let mut total_size = 0;
    for file in &files {
        total_size += file.size;
        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
        if !sources.iter().any(|(_, text)| text.contains(name)) {
            unused.push(file.clone());
        }
    }

    let existing: BTreeSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let missing = missing_refs(paths, config, &sources, &existing);

    Ok(Report {
        total: files.len(),
        total_size,
        reclaimable: unused.iter().map(|f| f.size).sum(),
        unused,
        missing,
    })
}

/// 删除指定的媒体文件。路径必须落在资源目录内，越界直接报错。
pub fn remove(paths: &ProjectPaths, relative_paths: &[String]) -> Result<Removed> {
    let root = paths.assets.clone();
    let mut removed = Vec::new();
    let mut freed = 0;

    for relative in relative_paths {
        let absolute = relative
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .fold(paths.static_dir.clone(), |acc, s| acc.join(s));

        // 只允许删资源目录里的文件：这个接口会被界面与自动化调用，
        // 不能因为传进来一个 ../../ 就把站点源文件删了。
        if !under(&absolute, &root) {
            return Err(Error::Other(format!("{relative} 不在资源目录内，拒绝删除")));
        }
        if !absolute.is_file() {
            continue;
        }
        let size = std::fs::metadata(&absolute)
            .map(|m| m.len())
            .unwrap_or_default();
        std::fs::remove_file(&absolute).map_err(|e| Error::io(&absolute, e))?;
        freed += size;
        removed.push(relative.clone());
    }

    Ok(Removed { removed, freed })
}

/// 资源目录里的全部文件。目录不存在时返回空列表。
fn collect_media(paths: &ProjectPaths, config: &AssetsConfig) -> Result<Vec<MediaFile>> {
    if !paths.assets.is_dir() {
        return Ok(Vec::new());
    }
    let dir = config.normalized_dir();
    let prefix = config.url_prefix();

    let mut files = Vec::new();
    for entry in WalkDir::new(&paths.assets)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_to_assets = util::to_slash(
            entry
                .path()
                .strip_prefix(&paths.assets)
                .unwrap_or(entry.path()),
        );
        let size = entry.metadata().map(|m| m.len()).unwrap_or_default();
        files.push(MediaFile {
            path: join_slash(&dir, &relative_to_assets),
            url: format!("{prefix}{relative_to_assets}"),
            size,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// 读入所有可能引用资源的文本：内容、模板、主题。
fn collect_sources(paths: &ProjectPaths) -> Vec<(String, String)> {
    let mut sources = Vec::new();
    for root in [&paths.content, &paths.templates, &paths.theme] {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() || !is_text(entry.path()) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                let label = util::to_slash(
                    entry
                        .path()
                        .strip_prefix(&paths.root)
                        .unwrap_or(entry.path()),
                );
                sources.push((label, text));
            }
        }
    }
    sources
}

/// 从文本里挑出指向资源目录的地址，逐个确认文件在不在。
fn missing_refs(
    paths: &ProjectPaths,
    config: &AssetsConfig,
    sources: &[(String, String)],
    existing: &BTreeSet<&str>,
) -> Vec<MissingRef> {
    let prefix = config.url_prefix();
    let dir = config.normalized_dir();
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (label, text) in sources {
        for url in extract_urls(text, &prefix) {
            let tail = url.trim_start_matches(&prefix);
            let relative = join_slash(&dir, tail);
            if existing.contains(relative.as_str()) {
                continue;
            }
            // 资源目录之外的静态文件不在体检范围内，但同名文件可能就在 static/ 下
            let absolute = relative
                .split('/')
                .fold(paths.static_dir.clone(), |acc, s| acc.join(s));
            if absolute.is_file() {
                continue;
            }
            found.entry(url).or_default().insert(label.clone());
        }
    }

    found
        .into_iter()
        .map(|(url, by)| MissingRef {
            url,
            referenced_by: by.into_iter().collect(),
        })
        .collect()
}

/// 抓出以 `prefix` 开头的地址。到引号、括号、空白或反引号为止。
fn extract_urls(text: &str, prefix: &str) -> BTreeSet<String> {
    let mut urls = BTreeSet::new();
    if prefix.is_empty() {
        return urls;
    }
    let mut rest = text;
    while let Some(at) = rest.find(prefix) {
        let candidate = &rest[at..];
        let end = candidate
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '(' | '`' | '>' | '<' | ']')
            })
            .unwrap_or(candidate.len());
        let url = candidate[..end].trim_end_matches(['.', ',', ';', ':']);
        if url.len() > prefix.len() {
            urls.insert(url.to_string());
        }
        rest = &candidate[end.max(1)..];
    }
    urls
}

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn join_slash(dir: &str, rest: &str) -> String {
    [dir, rest]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// `path` 是否位于 `root` 之内。两边都尽量 canonicalize，避免符号链接绕过。
fn under(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Assets, Build};

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: ProjectPaths,
        config: Assets,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let build = Build::default();
        let config = Assets::default();
        let paths = ProjectPaths::new(root, &build, &config);

        for d in [
            &paths.content,
            &paths.templates,
            &paths.theme,
            &paths.assets,
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        Fixture {
            _dir: dir,
            paths,
            config,
        }
    }

    fn write(path: &Path, name: &str, text: &str) {
        let target = path.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, text).unwrap();
    }

    #[test]
    fn unused_files_are_listed_and_referenced_ones_are_not() {
        let f = fixture();
        write(&f.paths.assets, "ab/used.png", "x");
        write(&f.paths.assets, "cd/orphan.png", "yy");
        write(
            &f.paths.content,
            "posts/a.md",
            "+++\ntitle = \"a\"\n+++\n\n![图](/images/ab/used.png)\n",
        );

        let report = audit(&f.paths, &f.config).unwrap();
        assert_eq!(report.total, 2);
        assert_eq!(report.unused.len(), 1);
        assert_eq!(report.unused[0].path, "images/cd/orphan.png");
        assert_eq!(report.unused[0].url, "/images/cd/orphan.png");
        assert_eq!(report.reclaimable, 2);
    }

    #[test]
    fn references_from_templates_and_themes_count() {
        let f = fixture();
        write(&f.paths.assets, "logo.png", "x");
        write(&f.paths.assets, "bg.png", "x");
        write(
            &f.paths.templates,
            "components/header.html",
            "<img src=\"/images/logo.png\" />",
        );
        write(
            &f.paths.theme,
            "static/css/main.css",
            "body { background-image: url(/images/bg.png); }",
        );

        let report = audit(&f.paths, &f.config).unwrap();
        assert!(
            report.unused.is_empty(),
            "模板与主题里的引用也算：{:?}",
            report.unused
        );
    }

    #[test]
    fn missing_references_point_at_the_files_that_use_them() {
        let f = fixture();
        write(
            &f.paths.content,
            "posts/a.md",
            "![图](/images/gone.png) 与 ![图2](/images/also-gone.png)",
        );
        write(
            &f.paths.templates,
            "pages/post.html",
            "<img src=\"/images/gone.png\">",
        );

        let report = audit(&f.paths, &f.config).unwrap();
        let urls: Vec<&str> = report.missing.iter().map(|m| m.url.as_str()).collect();
        assert_eq!(urls, vec!["/images/also-gone.png", "/images/gone.png"]);

        let gone = report
            .missing
            .iter()
            .find(|m| m.url.ends_with("gone.png"))
            .unwrap();
        assert_eq!(gone.referenced_by.len(), 1, "{gone:?}");
    }

    #[test]
    fn remove_deletes_only_inside_the_assets_dir() {
        let f = fixture();
        write(&f.paths.assets, "orphan.png", "yy");
        write(&f.paths.static_dir, "robots.txt", "User-agent: *");

        let removed = remove(&f.paths, &["images/orphan.png".to_string()]).unwrap();
        assert_eq!(removed.removed, vec!["images/orphan.png".to_string()]);
        assert_eq!(removed.freed, 2);
        assert!(!f.paths.assets.join("orphan.png").exists());

        // 资源目录之外的文件不能删
        let err = remove(&f.paths, &["robots.txt".to_string()]).unwrap_err();
        assert!(err.to_string().contains("拒绝删除"));
        assert!(f.paths.static_dir.join("robots.txt").exists());
    }

    #[test]
    fn traversal_attempts_are_rejected() {
        let f = fixture();
        write(&f.paths.content, "posts/a.md", "内容");
        // ../ 被剔除后落到 static_dir 里，仍在资源目录之外，因此报错
        let err = remove(&f.paths, &["../../content/posts/a.md".to_string()]).unwrap_err();
        assert!(err.to_string().contains("拒绝删除"));
        assert!(f.paths.content.join("posts/a.md").exists());
    }

    #[test]
    fn extract_urls_stops_at_delimiters() {
        let urls = extract_urls(
            "![a](/images/x.png) <img src=\"/images/y.png\"> url(/images/z.png);",
            "/images/",
        );
        let list: Vec<&str> = urls.iter().map(String::as_str).collect();
        assert_eq!(
            list,
            vec!["/images/x.png", "/images/y.png", "/images/z.png"]
        );
    }

    #[test]
    fn empty_assets_dir_is_not_an_error() {
        let f = fixture();
        std::fs::remove_dir_all(&f.paths.assets).unwrap();
        let report = audit(&f.paths, &f.config).unwrap();
        assert_eq!(report.total, 0);
        assert!(report.unused.is_empty());
    }
}
