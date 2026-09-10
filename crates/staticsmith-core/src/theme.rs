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

/// 单个条目解压后的上限。
///
/// 主题包装的是模板与样式，最大的东西通常是一张字体或背景图。给到 8 MB 已经很宽，
/// 而没有上限就意味着十几 KB 的包能声明出几 GB 的解压体积（zip bomb），
/// `read_to_end` 会照着展开——内存或磁盘先撑不住。
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// 整包解压后的上限。
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// 条目数上限。几万个空文件不占体积，但同样能把写盘和界面拖死。
const MAX_ENTRIES: usize = 2000;

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
    check_limits(&mut zip, archive)?;
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
    check_limits(&mut zip, archive)?;
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
        // 用 `take` 兜住：上面按声明的大小拦过一遍，但声明可以撒谎，
        // 真正读的时候还得有个硬顶。
        entry
            .by_ref()
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut buffer)
            .map_err(|e| Error::io(archive, e))?;
        if buffer.len() as u64 > MAX_ENTRY_BYTES {
            return Err(oversized_entry(archive, &name));
        }
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
    // Windows 的设备名：写入「成功」而内容进虚空，装完主题看着没报错但文件是空的。
    if let Some(bad) = segments.iter().find(|s| util::is_reserved_name(s)) {
        return Err(format!("{bad} 是系统保留名，装进来会写不成文件"));
    }
    let target = segments.iter().fold(base.clone(), |acc, s| acc.join(s));
    // 目标目录里可能已经有一条指向别处的符号链接（比如 `templates/layouts` 被人做成
    // 了链接）。`fs::write` 会跟着链接写过去，字面校验看不出来。
    if !util::is_within_resolved(base, &target) {
        return Err("路径经由符号链接走出了目标目录".to_string());
    }
    Ok(target)
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

fn oversized_entry(archive: &Path, entry: &str) -> Error {
    Error::Other(format!(
        "主题包里的 {entry} 解压后过大（超过 {} MB），已中止（{}）",
        MAX_ENTRY_BYTES / 1024 / 1024,
        archive.display()
    ))
}

/// 解压前先看一眼规模，超限就一个字节都不写。
///
/// 三道上限（单条目、整包、条目数）都按 zip 自己声明的解压后大小算——这是**免费**的，
/// 不用真解压。声明可以撒谎，所以写盘那一步还用 `take` 兜了一道硬顶。
///
/// 为什么要在 `scan` 里也查：装主题的流程是「先扫后装」，扫描阶段就会被界面调用，
/// 只在 `import` 拦的话，一个恶意包在预览时就能把内存吃光。
fn check_limits<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, archive: &Path) -> Result<()> {
    if zip.len() > MAX_ENTRIES {
        return Err(Error::Other(format!(
            "主题包里的条目太多（{} 个，上限 {MAX_ENTRIES}），已中止（{}）",
            zip.len(),
            archive.display()
        )));
    }

    let mut total: u64 = 0;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|e| zip_error(archive, e))?;
        if entry.size() > MAX_ENTRY_BYTES {
            let name = entry.name().to_string();
            return Err(oversized_entry(archive, &name));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_BYTES {
            return Err(Error::Other(format!(
                "主题包解压后过大（超过 {} MB），已中止（{}）",
                MAX_TOTAL_BYTES / 1024 / 1024,
                archive.display()
            )));
        }
    }
    Ok(())
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

    /// 高压缩比的包（zip bomb）要在解压前就被拦下。
    ///
    /// 十几 KB 的包能声明出几 GB 的解压后体积，`read_to_end` 会照着展开——
    /// 内存或磁盘先撑不住。主题包装的是模板与样式，本来就不该有巨大文件。
    #[test]
    fn an_oversized_entry_is_rejected_before_unpacking() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("bomb.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default();
            writer.start_file(MANIFEST_NAME, options).unwrap();
            writer.write_all("name = \"炸弹\"\n".as_bytes()).unwrap();
            writer
                .start_file("templates/layouts/base.html", options)
                .unwrap();
            // 全零高度可压缩：包本身只有几 KB
            writer
                .write_all(&vec![0u8; MAX_ENTRY_BYTES as usize + 1])
                .unwrap();
            writer.finish().unwrap();
        }
        assert!(
            std::fs::metadata(&zip_path).unwrap().len() < 100 * 1024,
            "这个包本身应该很小，否则测的就不是压缩比了"
        );

        let site = fixture();
        let err = scan(&site.paths, &zip_path).unwrap_err().to_string();
        assert!(err.contains("过大"), "{err}");

        let err = import(&site.paths, &zip_path, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("过大"), "{err}");
        assert!(
            !site.paths.templates.join("layouts/base.html").exists(),
            "拦下之后不该留下半个文件"
        );
    }

    /// 条目数上限：几万个空文件同样能把界面和磁盘拖死。
    #[test]
    fn too_many_entries_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("many.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::SimpleFileOptions::default();
            writer.start_file(MANIFEST_NAME, options).unwrap();
            writer.write_all("name = \"很多\"\n".as_bytes()).unwrap();
            for i in 0..=MAX_ENTRIES {
                writer
                    .start_file(format!("templates/pages/p{i}.html"), options)
                    .unwrap();
                writer.write_all(b"x").unwrap();
            }
            writer.finish().unwrap();
        }

        let site = fixture();
        let err = scan(&site.paths, &zip_path).unwrap_err().to_string();
        assert!(err.contains("条目"), "{err}");
    }

    /// 条目名是 Windows 保留设备名时也要拒绝：写入「成功」而内容进虚空，
    /// 装完看着没报错，文件却是空的。
    #[test]
    fn reserved_device_names_are_rejected_too() {
        let site = fixture();
        assert!(target_of(&site.paths, "templates/nul.html").is_err());
        assert!(target_of(&site.paths, "theme/static/com1.css").is_err());
        // 别过度拦：只是包含保留名的普通文件
        assert!(target_of(&site.paths, "templates/console.html").is_ok());
    }

    /// 目标目录里已有的符号链接不能变成出口。
    ///
    /// `fs::write` 会跟着链接写过去，而条目名本身完全合法——这一条只有解析之后
    /// 才看得出来。环境不允许建链接时跳过（Windows 需要开发者模式）。
    #[test]
    fn a_symlinked_target_dir_is_rejected() {
        let site = fixture();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(&site.paths.templates).unwrap();
        if !util::try_symlink_dir(outside.path(), &site.paths.templates.join("layouts")) {
            eprintln!("跳过：当前环境不允许建符号链接");
            return;
        }

        assert!(
            target_of(&site.paths, "templates/layouts/base.html").is_err(),
            "经由符号链接写到模板目录之外，应当被拒绝"
        );
        assert!(target_of(&site.paths, "templates/pages/post.html").is_ok());
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
