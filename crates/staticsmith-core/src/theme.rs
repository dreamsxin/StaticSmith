//! 主题包：把站点的外观打成一个 zip，也能装回另一个站点。
//!
//! 换外观此前只能手动拷目录：`templates/` 一份、`themes/<主题>/` 一份，漏一个就渲染失败。
//! 这个模块把两者收进一个 zip，附一张 `theme.toml` 说明是什么、谁做的。
//!
//! 三条约定，都是为了「装主题不该毁掉我的站」：
//!
//! - **只碰外观**：`content/` 与 `static/` 一个字节都不动。文章与上传的图片是站点的，
//!   不是主题的；把它们打进包里，装一次主题就会覆盖别人的内容。
//! - **先扫后装**：`scan` 只读，把「会写哪些文件、其中哪些已存在」列清楚，
//!   由调用方决定是跳过还是覆盖。默认跳过。
//! - **不信任压缩包里的路径**：`..`、绝对路径、盘符与反斜杠开头一律拒绝
//!   （zip slip），越界的条目在扫描阶段就被剔掉，不会等到写盘。

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::ProjectPaths;
use crate::error::{Error, Result};
use crate::util;

/// 包内的说明文件名。
pub const MANIFEST_NAME: &str = "theme.toml";

/// 包内模板目录前缀。
const TEMPLATES_PREFIX: &str = "templates/";

/// 包内主题静态资源前缀。
///
/// 刻意不叫 `themes/default/`：装包的站点可能把 `theme_dir` 配成别的名字，
/// 包里写死目录名就会装到错的地方。
const THEME_PREFIX: &str = "theme/";

/// 主题包的说明。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
}

/// 打包结果。
#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    /// 写出的 zip 路径。
    pub archive: PathBuf,
    /// 打进包里的文件数（不含说明文件）。
    pub files: usize,
    pub templates: usize,
    pub assets: usize,
}

/// 装包前的干跑结果。
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub manifest: Manifest,
    /// 会写出的文件（相对站点根目录，正斜杠）。
    pub files: Vec<String>,
    /// 其中已经存在、装包会覆盖的那些。
    pub conflicts: Vec<String>,
    /// 被拒绝的条目及原因（越界路径、不认识的目录前缀）。
    pub rejected: Vec<Rejected>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rejected {
    /// 压缩包里的原始条目名。
    pub entry: String,
    pub reason: String,
}

/// 装包结果。
#[derive(Debug, Clone, Serialize)]
pub struct Imported {
    pub manifest: Manifest,
    /// 真正写盘的文件。
    pub written: Vec<String>,
    /// 已存在而跳过的文件（`overwrite` 为假时）。
    pub skipped: Vec<String>,
    pub rejected: Vec<Rejected>,
}

/// 把当前站点的模板与主题静态资源打成一个 zip。
pub fn export(paths: &ProjectPaths, archive: &Path, manifest: &Manifest) -> Result<Exported> {
    if manifest.name.trim().is_empty() {
        return Err(Error::Other("主题名不能为空".to_string()));
    }
    if let Some(parent) = archive.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
    }

    let file = std::fs::File::create(archive).map_err(|e| Error::io(archive, e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let raw = toml::to_string_pretty(manifest).map_err(|e| Error::Other(e.to_string()))?;
    writer
        .start_file(MANIFEST_NAME, options)
        .map_err(|e| zip_error(archive, e))?;
    writer
        .write_all(raw.as_bytes())
        .map_err(|e| Error::io(archive, e))?;

    let templates = add_tree(
        &mut writer,
        archive,
        &paths.templates,
        TEMPLATES_PREFIX,
        options,
    )?;
    let assets = add_tree(&mut writer, archive, &paths.theme, THEME_PREFIX, options)?;
    writer.finish().map_err(|e| zip_error(archive, e))?;

    Ok(Exported {
        archive: archive.to_path_buf(),
        files: templates + assets,
        templates,
        assets,
    })
}

/// 读出主题包会做什么，不写任何文件。
pub fn scan(paths: &ProjectPaths, archive: &Path) -> Result<Preview> {
    let mut zip = open(archive)?;
    let manifest = read_manifest(&mut zip, archive)?;

    let mut files = Vec::new();
    let mut conflicts = Vec::new();
    let mut rejected = Vec::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| zip_error(archive, e))?;
        let name = entry.name().to_string();
        if entry.is_dir() || name == MANIFEST_NAME {
            continue;
        }
        match target_of(paths, &name) {
            Ok(target) => {
                let relative = util::to_slash(target.strip_prefix(&paths.root).unwrap_or(&target));
                if target.exists() {
                    conflicts.push(relative.clone());
                }
                files.push(relative);
            }
            Err(reason) => rejected.push(Rejected {
                entry: name,
                reason,
            }),
        }
    }
    files.sort();
    conflicts.sort();

    Ok(Preview {
        manifest,
        files,
        conflicts,
        rejected,
    })
}

/// 装主题包。`overwrite` 为假时已存在的文件一律跳过。
///
/// 只写 `templates/` 与主题目录；`content/`、`static/` 与 `staticsmith.toml`
/// 不在写入范围内——换外观不该动内容，也不该顺手改站点配置。
pub fn import(paths: &ProjectPaths, archive: &Path, overwrite: bool) -> Result<Imported> {
    let mut zip = open(archive)?;
    let manifest = read_manifest(&mut zip, archive)?;

    let mut written = Vec::new();
    let mut skipped = Vec::new();
    let mut rejected = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| zip_error(archive, e))?;
        let name = entry.name().to_string();
        if entry.is_dir() || name == MANIFEST_NAME {
            continue;
        }
        let target = match target_of(paths, &name) {
            Ok(target) => target,
            Err(reason) => {
                rejected.push(Rejected {
                    entry: name,
                    reason,
                });
                continue;
            }
        };
        let relative = util::to_slash(target.strip_prefix(&paths.root).unwrap_or(&target));
        if target.exists() && !overwrite {
            skipped.push(relative);
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .map_err(|e| Error::io(archive, e))?;
        std::fs::write(&target, buffer).map_err(|e| Error::io(&target, e))?;
        written.push(relative);
    }
    written.sort();
    skipped.sort();

    Ok(Imported {
        manifest,
        written,
        skipped,
        rejected,
    })
}

/// 把包内条目名映射成站点里的绝对路径，同时挡掉越界。
///
/// 校验在字符串上做：目标文件还不存在，`canonicalize` 会失败；而 Windows 上
/// `PathBuf::push("C:\\x")` 会替换整条路径，必须在 `join` 之前拦住。
fn target_of(paths: &ProjectPaths, entry: &str) -> std::result::Result<PathBuf, String> {
    let normalized = entry.replace('\\', "/");
    let (base, rest) = if let Some(rest) = normalized.strip_prefix(TEMPLATES_PREFIX) {
        (&paths.templates, rest)
    } else if let Some(rest) = normalized.strip_prefix(THEME_PREFIX) {
        (&paths.theme, rest)
    } else {
        return Err(format!(
            "不认识的位置（只装 {TEMPLATES_PREFIX} 与 {THEME_PREFIX}）"
        ));
    };

    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err("条目名为空".to_string());
    }
    if segments
        .iter()
        .any(|s| *s == ".." || *s == "." || s.contains(':'))
    {
        return Err("路径越界".to_string());
    }
    Ok(segments.iter().fold(base.clone(), |acc, s| acc.join(s)))
}

fn open(archive: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(archive).map_err(|e| Error::io(archive, e))?;
    zip::ZipArchive::new(file).map_err(|e| zip_error(archive, e))
}

fn read_manifest<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, archive: &Path) -> Result<Manifest> {
    let mut entry = zip.by_name(MANIFEST_NAME).map_err(|_| {
        Error::Other(format!(
            "{} 不像主题包：缺少 {MANIFEST_NAME}",
            archive.display()
        ))
    })?;
    let mut raw = String::new();
    entry
        .read_to_string(&mut raw)
        .map_err(|e| Error::io(archive, e))?;
    let manifest: Manifest =
        toml::from_str(&raw).map_err(|e| Error::Other(format!("{MANIFEST_NAME} 解析失败: {e}")))?;
    if manifest.name.trim().is_empty() {
        return Err(Error::Other(format!("{MANIFEST_NAME} 里缺少主题名")));
    }
    Ok(manifest)
}

/// 把一个目录整棵塞进 zip。目录不存在时当成 0 个文件，不报错。
fn add_tree<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    archive: &Path,
    dir: &Path,
    prefix: &str,
    options: zip::write::FileOptions<'_, ()>,
) -> Result<usize> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    // 排序后再写：同一份目录打两次包应该得到相同的条目顺序
    let mut entries: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();
    entries.sort();

    for path in entries {
        let relative = util::to_slash(path.strip_prefix(dir).unwrap_or(&path));
        writer
            .start_file(format!("{prefix}{relative}"), options)
            .map_err(|e| zip_error(archive, e))?;
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        writer
            .write_all(&bytes)
            .map_err(|e| Error::io(archive, e))?;
        count += 1;
    }
    Ok(count)
}

fn zip_error(archive: &Path, source: zip::result::ZipError) -> Error {
    Error::Other(format!("主题包读写失败（{}）: {source}", archive.display()))
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
        Fixture { _dir: dir, paths }
    }

    fn write(root: &Path, relative: &str, raw: &str) {
        let path = relative
            .split('/')
            .fold(root.to_path_buf(), |acc, s| acc.join(s));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, raw).unwrap();
    }

    fn read(root: &Path, relative: &str) -> String {
        let path = relative
            .split('/')
            .fold(root.to_path_buf(), |acc, s| acc.join(s));
        std::fs::read_to_string(path).unwrap()
    }

    fn manifest() -> Manifest {
        Manifest {
            name: "极简".to_string(),
            version: "1.0.0".to_string(),
            description: "两栏，无 JS".to_string(),
            author: "我".to_string(),
        }
    }

    #[test]
    fn export_then_import_moves_the_whole_look() {
        let from = fixture();
        write(
            &from.paths.templates,
            "layouts/base.html",
            "<html>{% block main %}{% endblock %}</html>",
        );
        write(&from.paths.templates, "pages/post.html", "文章模板");
        write(&from.paths.theme, "static/css/main.css", "body{}");
        // 内容与上传的图片不属于主题，不该进包
        write(
            &from.paths.content,
            "posts/a.md",
            "+++\ntitle = \"甲\"\n+++\n",
        );
        write(&from.paths.assets, "photo.png", "PNG");

        let zip_path = from.paths.root.join("dist/极简.zip");
        let report = export(&from.paths, &zip_path, &manifest()).unwrap();
        assert_eq!(report.templates, 2);
        assert_eq!(report.assets, 1);
        assert_eq!(report.files, 3);

        let to = fixture();
        let preview = scan(&to.paths, &zip_path).unwrap();
        assert_eq!(preview.manifest.name, "极简");
        assert_eq!(preview.files.len(), 3, "{:?}", preview.files);
        assert!(preview.conflicts.is_empty(), "空站点没有冲突");
        assert!(preview.rejected.is_empty());
        assert!(
            !preview.files.iter().any(|f| f.contains("content")),
            "内容不该出现在清单里：{:?}",
            preview.files
        );

        let done = import(&to.paths, &zip_path, false).unwrap();
        assert_eq!(done.written.len(), 3);
        assert!(done.skipped.is_empty());
        assert_eq!(
            read(&to.paths.templates, "layouts/base.html"),
            "<html>{% block main %}{% endblock %}</html>"
        );
        assert_eq!(read(&to.paths.theme, "static/css/main.css"), "body{}");
        assert!(
            !to.paths.content.join("posts/a.md").exists(),
            "内容没被带过去"
        );
    }

    #[test]
    fn import_skips_existing_files_unless_told_to_overwrite() {
        let from = fixture();
        write(&from.paths.templates, "layouts/base.html", "新的");
        let zip_path = from.paths.root.join("theme.zip");
        export(&from.paths, &zip_path, &manifest()).unwrap();

        let to = fixture();
        write(&to.paths.templates, "layouts/base.html", "我自己改过的");

        // 冲突要在干跑里就看得见
        let preview = scan(&to.paths, &zip_path).unwrap();
        assert_eq!(preview.conflicts.len(), 1, "{:?}", preview.conflicts);

        let done = import(&to.paths, &zip_path, false).unwrap();
        assert!(done.written.is_empty());
        assert_eq!(done.skipped.len(), 1);
        assert_eq!(
            read(&to.paths.templates, "layouts/base.html"),
            "我自己改过的"
        );

        let done = import(&to.paths, &zip_path, true).unwrap();
        assert_eq!(done.written.len(), 1);
        assert_eq!(read(&to.paths.templates, "layouts/base.html"), "新的");
    }

    #[test]
    fn entries_outside_the_two_directories_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default();
            writer.start_file(MANIFEST_NAME, options).unwrap();
            writer.write_all("name = \"坏包\"\n".as_bytes()).unwrap();
            for entry in [
                "templates/../../../etc/passwd",
                "theme/..\\..\\windows\\system32\\evil.dll",
                "content/posts/hijacked.md",
                "staticsmith.toml",
            ] {
                writer.start_file(entry, options).unwrap();
                writer.write_all(b"x").unwrap();
            }
            writer.finish().unwrap();
        }

        let site = fixture();
        let preview = scan(&site.paths, &zip_path).unwrap();
        assert!(preview.files.is_empty(), "{:?}", preview.files);
        assert_eq!(preview.rejected.len(), 4, "{:?}", preview.rejected);

        let done = import(&site.paths, &zip_path, true).unwrap();
        assert!(done.written.is_empty());
        assert_eq!(done.rejected.len(), 4);
        assert!(!site.paths.root.join("staticsmith.toml").exists());
        assert!(!site.paths.content.join("posts/hijacked.md").exists());
    }

    #[test]
    fn a_zip_without_manifest_is_not_a_theme_package() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("plain.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default();
            writer
                .start_file("templates/layouts/base.html", options)
                .unwrap();
            writer.write_all(b"x").unwrap();
            writer.finish().unwrap();
        }

        let site = fixture();
        let err = scan(&site.paths, &zip_path).unwrap_err();
        assert!(err.to_string().contains(MANIFEST_NAME), "{err}");
    }

    #[test]
    fn export_refuses_an_unnamed_theme() {
        let site = fixture();
        let err = export(
            &site.paths,
            &site.paths.root.join("x.zip"),
            &Manifest::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("主题名"), "{err}");
    }
}
