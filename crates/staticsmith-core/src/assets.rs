//! 媒体资源的内容寻址存储。
//!
//! 编辑器里粘贴或拖入图片时，文件按内容哈希命名后写入 `static_dir/<assets.dir>/`。
//! 这样做的收益：
//!
//! - **天然去重**：同一张图在多篇文章里粘贴多次，磁盘上只有一份
//! - **可缓存**：内容变了文件名就变，CDN / 浏览器可以按永久缓存处理
//! - **无冲突**：不依赖用户给的文件名，剪贴板里那种 `image.png` 不会互相覆盖
//!
//! 资源目录必须位于 `static_dir` 之内，因此「构建时复制到产物」与「页面里的 URL」
//! 由同一份相对路径推导，不存在写入位置与引用地址不一致的可能。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{AssetNaming, Assets as AssetsConfig};
use crate::error::{Error, Result};

/// 一次保存的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedAsset {
    /// 内容哈希全量十六进制（始终为 SHA-256，与命名策略无关，用于索引去重）。
    pub content_hash: String,
    /// 最终文件名。
    pub file_name: String,
    /// 相对 `static_dir` 的路径，正斜杠。
    pub relative_path: String,
    /// 站内引用地址，可直接写进 Markdown。
    pub url: String,
    pub size: u64,
    /// 命中已有文件因而没有实际写盘。
    pub deduplicated: bool,
}

/// 资源写入器。借用配置与路径，本身无状态。
pub struct AssetStore<'a> {
    static_dir: &'a Path,
    config: &'a AssetsConfig,
}

impl<'a> AssetStore<'a> {
    pub fn new(static_dir: &'a Path, config: &'a AssetsConfig) -> Self {
        Self { static_dir, config }
    }

    /// 保存字节流。`original_name` 只用于推断扩展名与 `original` 命名策略，
    /// 其中的目录部分会被丢弃，不可能写到资源目录之外。
    pub fn save(&self, bytes: &[u8], original_name: &str) -> Result<SavedAsset> {
        self.check_size(bytes.len())?;

        let content_hash = crate::util::hash_bytes_full(bytes);
        let extension = extension_of(original_name)
            .or_else(|| sniff_extension(bytes).map(str::to_string))
            .unwrap_or_else(|| "bin".to_string());

        let naming_hash = match self.config.naming {
            AssetNaming::Sha256 => content_hash.clone(),
            AssetNaming::Md5 => md5_hex(bytes),
            AssetNaming::Original => content_hash.clone(),
        };
        let short = &naming_hash[..self.config.effective_hash_length().min(naming_hash.len())];

        let (file_name, shard) = match self.config.naming {
            AssetNaming::Original => {
                let stem = sanitize_stem(original_name);
                let stem = if stem.is_empty() {
                    short.to_string()
                } else {
                    stem
                };
                (format!("{stem}.{extension}"), None)
            }
            _ => (
                format!("{short}.{extension}"),
                self.config.shard.then(|| short[..2].to_string()),
            ),
        };

        let mut relative = self.relative_path(&file_name, shard.as_deref());
        let mut absolute = self.absolute(&relative);

        // 已存在同名文件：内容相同则复用，不同则说明是 original 命名撞名，追加短哈希。
        if absolute.is_file() {
            if same_content(&absolute, bytes)? {
                return Ok(SavedAsset {
                    size: bytes.len() as u64,
                    url: self.url_for(&file_name, shard.as_deref()),
                    content_hash,
                    file_name,
                    relative_path: relative,
                    deduplicated: true,
                });
            }
            let stem = file_name
                .rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or_else(|| file_name.clone());
            let unique = format!("{stem}-{}.{extension}", &short[..8]);
            relative = self.relative_path(&unique, shard.as_deref());
            absolute = self.absolute(&relative);
            let url = self.url_for(&unique, shard.as_deref());
            self.write(&absolute, bytes)?;
            return Ok(SavedAsset {
                content_hash,
                file_name: unique,
                relative_path: relative,
                url,
                size: bytes.len() as u64,
                deduplicated: false,
            });
        }

        self.write(&absolute, bytes)?;
        Ok(SavedAsset {
            url: self.url_for(&file_name, shard.as_deref()),
            content_hash,
            file_name,
            relative_path: relative,
            size: bytes.len() as u64,
            deduplicated: false,
        })
    }

    /// 资源目录的绝对路径。
    pub fn dir(&self) -> PathBuf {
        self.config
            .normalized_dir()
            .split('/')
            .filter(|s| !s.is_empty())
            .fold(self.static_dir.to_path_buf(), |acc, s| acc.join(s))
    }

    fn relative_path(&self, file_name: &str, shard: Option<&str>) -> String {
        let dir = self.config.normalized_dir();
        [dir.as_str(), shard.unwrap_or(""), file_name]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn url_for(&self, file_name: &str, shard: Option<&str>) -> String {
        let rest = [shard.unwrap_or(""), file_name]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        format!("{}{}", self.config.url_prefix(), rest)
    }

    fn absolute(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.static_dir.to_path_buf(), |acc, s| acc.join(s))
    }

    fn check_size(&self, len: usize) -> Result<()> {
        let limit = self.config.max_size_mb;
        if limit > 0 && len as u64 > limit * 1024 * 1024 {
            return Err(Error::Other(format!(
                "资源大小 {:.1} MB 超过上限 {} MB（assets.max_size_mb）",
                len as f64 / 1024.0 / 1024.0,
                limit
            )));
        }
        Ok(())
    }

    fn write(&self, absolute: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = absolute.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        std::fs::write(absolute, bytes).map_err(|e| Error::io(absolute, e))
    }
}

fn same_content(path: &Path, bytes: &[u8]) -> Result<bool> {
    let existing = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(existing == bytes)
}

fn md5_hex(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 从用户给的文件名里取扩展名，严格清洗：只保留 ASCII 字母数字，最长 8 位。
///
/// 这是外部输入进入文件系统的唯一通道，宽松处理会带来路径穿越与奇怪后缀的风险。
fn extension_of(original_name: &str) -> Option<String> {
    let name = original_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(original_name);
    let ext = name.rsplit_once('.')?.1;
    let cleaned: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// `original` 命名策略下清洗文件主干：保留字母数字（含中日韩）与 `-` `_`，其余折叠为 `-`。
fn sanitize_stem(original_name: &str) -> String {
    let name = original_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(original_name);
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.chars().count() >= 64 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// 剪贴板粘贴的图片往往没有文件名，按魔术字节判断类型。
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("bmp");
    }
    if bytes.starts_with(b"%PDF") {
        return Some("pdf");
    }
    let head = &bytes[..bytes.len().min(256)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    if trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && text.contains("<svg")) {
        return Some("svg");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\x00fake png payload";

    #[test]
    fn saves_under_hashed_shard_and_returns_matching_url() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig::default();
        let store = AssetStore::new(dir.path(), &config);

        let saved = store.save(PNG_BYTES, "屏幕截图 2026-09-07.PNG").unwrap();

        assert!(saved.file_name.ends_with(".png"), "{}", saved.file_name);
        assert_eq!(saved.file_name.len(), 16 + 4, "16 位哈希 + .png");
        assert_eq!(
            saved.relative_path,
            format!("images/{}/{}", &saved.file_name[..2], saved.file_name)
        );
        assert_eq!(saved.url, format!("/{}", saved.relative_path));
        assert!(!saved.deduplicated);
        assert_eq!(saved.size, PNG_BYTES.len() as u64);

        let on_disk = saved
            .relative_path
            .split('/')
            .fold(dir.path().to_path_buf(), |acc, s| acc.join(s));
        assert!(on_disk.is_file(), "文件应落在 {}", on_disk.display());
    }

    #[test]
    fn identical_bytes_are_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig::default();
        let store = AssetStore::new(dir.path(), &config);

        let first = store.save(PNG_BYTES, "a.png").unwrap();
        let second = store.save(PNG_BYTES, "完全不同的名字.png").unwrap();

        assert_eq!(first.relative_path, second.relative_path);
        assert!(!first.deduplicated);
        assert!(second.deduplicated, "第二次应命中已有文件");
    }

    #[test]
    fn different_bytes_get_different_names() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig::default();
        let store = AssetStore::new(dir.path(), &config);

        let a = store.save(PNG_BYTES, "a.png").unwrap();
        let b = store
            .save(b"\x89PNG\r\n\x1a\n other bytes", "a.png")
            .unwrap();
        assert_ne!(a.file_name, b.file_name);
    }

    #[test]
    fn md5_naming_produces_a_different_name_than_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let sha = AssetsConfig::default();
        let md5 = AssetsConfig {
            naming: AssetNaming::Md5,
            ..AssetsConfig::default()
        };

        let a = AssetStore::new(dir.path(), &sha)
            .save(PNG_BYTES, "a.png")
            .unwrap();
        let b = AssetStore::new(dir.path(), &md5)
            .save(PNG_BYTES, "a.png")
            .unwrap();

        assert_ne!(a.file_name, b.file_name);
        // content_hash 始终是 SHA-256，便于索引层统一去重。
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.content_hash.len(), 64);
    }

    #[test]
    fn original_naming_keeps_the_name_and_disambiguates_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig {
            naming: AssetNaming::Original,
            ..AssetsConfig::default()
        };
        let store = AssetStore::new(dir.path(), &config);

        let first = store.save(PNG_BYTES, "Cover Image.png").unwrap();
        assert_eq!(first.file_name, "cover-image.png");
        assert_eq!(first.relative_path, "images/cover-image.png");

        let same = store.save(PNG_BYTES, "cover image.png").unwrap();
        assert!(same.deduplicated);

        let other = store
            .save(b"\x89PNG\r\n\x1a\n different", "Cover Image.png")
            .unwrap();
        assert!(other.file_name.starts_with("cover-image-"));
        assert!(!other.deduplicated);
    }

    #[test]
    fn path_traversal_in_the_file_name_is_neutralized() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig {
            naming: AssetNaming::Original,
            ..AssetsConfig::default()
        };
        let store = AssetStore::new(dir.path(), &config);

        let saved = store.save(PNG_BYTES, "../../../etc/passwd.png").unwrap();
        assert_eq!(saved.file_name, "passwd.png");
        assert!(saved.relative_path.starts_with("images/"));
        assert!(dir.path().join("images").join("passwd.png").is_file());
    }

    #[test]
    fn extension_falls_back_to_content_sniffing() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig::default();
        let store = AssetStore::new(dir.path(), &config);

        // 剪贴板粘贴常见情形：没有文件名与后缀。
        let saved = store.save(PNG_BYTES, "").unwrap();
        assert!(saved.file_name.ends_with(".png"));

        let unknown = store.save(b"just some bytes", "noext").unwrap();
        assert!(unknown.file_name.ends_with(".bin"));
    }

    #[test]
    fn sniffs_common_image_formats() {
        assert_eq!(sniff_extension(PNG_BYTES), Some("png"));
        assert_eq!(sniff_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(sniff_extension(b"GIF89a...."), Some("gif"));
        assert_eq!(sniff_extension(b"RIFF____WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_extension(b"<svg xmlns=\"...\">"), Some("svg"));
        assert_eq!(sniff_extension(b"\x00\x01\x02"), None);
    }

    #[test]
    fn oversized_files_are_rejected_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig {
            max_size_mb: 1,
            ..AssetsConfig::default()
        };
        let store = AssetStore::new(dir.path(), &config);

        let big = vec![0u8; 2 * 1024 * 1024];
        let err = store.save(&big, "big.png").unwrap_err();
        assert!(err.to_string().contains("超过上限"));
        assert!(!dir.path().join("images").exists(), "拒绝时不应建目录");
    }

    #[test]
    fn shard_can_be_disabled_and_custom_url_prefix_applies() {
        let dir = tempfile::tempdir().unwrap();
        let config = AssetsConfig {
            dir: "media".into(),
            shard: false,
            url_prefix: Some("https://cdn.example.com/media".into()),
            ..AssetsConfig::default()
        };
        let store = AssetStore::new(dir.path(), &config);

        let saved = store.save(PNG_BYTES, "a.png").unwrap();
        assert_eq!(saved.relative_path, format!("media/{}", saved.file_name));
        assert_eq!(
            saved.url,
            format!("https://cdn.example.com/media/{}", saved.file_name)
        );
    }
}
