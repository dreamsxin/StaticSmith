//! 本地产物清单与差异同步计划。
//!
//! 这部分逻辑与具体协议无关，因此单独成模块并有完整单元测试：
//! FTP/SFTP 只需提供远端文件列表，就能复用同一套 `rsync` 风格比对规则。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{Error, Result};

/// 本地待上传文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEntry {
    /// 相对 `dist/` 的正斜杠路径。
    pub path: String,
    pub absolute: PathBuf,
    pub size: u64,
    /// Unix 时间戳（秒）。取不到时为 `None`。
    pub modified_epoch: Option<i64>,
}

/// 远端已存在的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub path: String,
    pub size: u64,
    pub modified_epoch: Option<i64>,
}

/// 差异同步计划。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncPlan {
    /// 需要上传（新增或变更）的文件。
    pub upload: Vec<String>,
    /// 远端多余、需要删除的文件。
    pub delete: Vec<String>,
    /// 内容一致因而跳过的文件数。
    pub skipped: usize,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.upload.is_empty() && self.delete.is_empty()
    }
}

/// 扫描输出目录，生成本地清单。
pub fn scan(dist_dir: &Path) -> Result<Vec<LocalEntry>> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(dist_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::io(entry.path(), std::io::Error::other(e.to_string())))?;
        let relative = entry.path().strip_prefix(dist_dir).unwrap_or(entry.path());
        entries.push(LocalEntry {
            path: to_slash(relative),
            absolute: entry.path().to_path_buf(),
            size: metadata.len(),
            modified_epoch: metadata.modified().ok().and_then(to_epoch),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// 比对本地与远端清单。
///
/// 判定为「需要上传」的条件：远端缺失、大小不同，或本地修改时间更新。
/// 两侧时间戳都存在时才比较时间，否则只看大小——FTP 服务器的时间精度差异很大。
pub fn plan_sync(local: &[LocalEntry], remote: &[RemoteEntry], delete_extra: bool) -> SyncPlan {
    let mut plan = SyncPlan::default();

    for item in local {
        match remote.iter().find(|r| r.path == item.path) {
            None => plan.upload.push(item.path.clone()),
            Some(r) if r.size != item.size => plan.upload.push(item.path.clone()),
            Some(r) => match (item.modified_epoch, r.modified_epoch) {
                // 容忍 2 秒误差：FAT/FTP 的时间戳精度只有 2 秒。
                (Some(l), Some(rm)) if l > rm + 2 => plan.upload.push(item.path.clone()),
                _ => plan.skipped += 1,
            },
        }
    }

    if delete_extra {
        for r in remote {
            if !local.iter().any(|l| l.path == r.path) {
                plan.delete.push(r.path.clone());
            }
        }
    }

    plan.upload.sort();
    plan.delete.sort();
    plan
}

/// 上传前需要在远端创建的目录，按层级由浅到深排序。
pub fn required_directories(paths: &[String]) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for path in paths {
        let segments: Vec<&str> = path.split('/').collect();
        for depth in 1..segments.len() {
            let dir = segments[..depth].join("/");
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    dirs.sort_by_key(|d| (d.matches('/').count(), d.clone()));
    dirs
}

fn to_slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn to_epoch(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(path: &str, size: u64, modified: Option<i64>) -> LocalEntry {
        LocalEntry {
            path: path.to_string(),
            absolute: PathBuf::from(path),
            size,
            modified_epoch: modified,
        }
    }

    fn remote(path: &str, size: u64, modified: Option<i64>) -> RemoteEntry {
        RemoteEntry {
            path: path.to_string(),
            size,
            modified_epoch: modified,
        }
    }

    #[test]
    fn uploads_missing_and_changed_files_only() {
        let local_files = vec![
            local("index.html", 100, Some(1000)),
            local("about/index.html", 50, Some(1000)),
            local("css/main.css", 20, Some(1000)),
        ];
        let remote_files = vec![
            remote("index.html", 100, Some(1000)),      // 完全一致 → 跳过
            remote("about/index.html", 40, Some(1000)), // 大小不同 → 上传
        ];

        let plan = plan_sync(&local_files, &remote_files, false);
        assert_eq!(plan.upload, vec!["about/index.html", "css/main.css"]);
        assert_eq!(plan.skipped, 1);
        assert!(plan.delete.is_empty());
    }

    #[test]
    fn newer_local_timestamp_triggers_upload() {
        let plan = plan_sync(
            &[local("a.html", 10, Some(2_000))],
            &[remote("a.html", 10, Some(1_000))],
            false,
        );
        assert_eq!(plan.upload, vec!["a.html"]);
    }

    #[test]
    fn two_second_clock_skew_is_tolerated() {
        let plan = plan_sync(
            &[local("a.html", 10, Some(1_002))],
            &[remote("a.html", 10, Some(1_000))],
            false,
        );
        assert!(plan.upload.is_empty());
        assert_eq!(plan.skipped, 1);
    }

    #[test]
    fn missing_timestamps_fall_back_to_size_comparison() {
        let plan = plan_sync(
            &[local("a.html", 10, None)],
            &[remote("a.html", 10, None)],
            false,
        );
        assert!(plan.upload.is_empty());
    }

    #[test]
    fn delete_extra_is_opt_in() {
        let local_files = vec![local("a.html", 1, None)];
        let remote_files = vec![remote("a.html", 1, None), remote("stale.html", 1, None)];

        assert!(plan_sync(&local_files, &remote_files, false)
            .delete
            .is_empty());
        assert_eq!(
            plan_sync(&local_files, &remote_files, true).delete,
            vec!["stale.html"]
        );
    }

    #[test]
    fn directories_are_created_shallow_first() {
        let dirs = required_directories(&[
            "posts/page/2/index.html".to_string(),
            "css/main.css".to_string(),
            "index.html".to_string(),
        ]);
        assert_eq!(dirs, vec!["css", "posts", "posts/page", "posts/page/2"]);
    }

    #[test]
    fn scan_reports_relative_slash_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("posts")).unwrap();
        std::fs::write(dir.path().join("index.html"), "a").unwrap();
        std::fs::write(dir.path().join("posts/index.html"), "bb").unwrap();

        let entries = scan(dir.path()).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            vec!["index.html", "posts/index.html"]
        );
        assert_eq!(entries[1].size, 2);
    }
}
