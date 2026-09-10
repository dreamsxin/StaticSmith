//! 本地产物清单与差异同步计划。
//!
//! 这部分逻辑与具体协议无关，因此单独成模块并有完整单元测试：
//! FTP/SFTP 只需提供远端文件列表，就能复用同一套 `rsync` 风格比对规则。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use staticsmith_core::config::FtpOverwrite;

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
/// 远端缺这个文件时一律上传，与规则无关——「跳过」说的是「不覆盖已有的」，
/// 不是「不发新文件」。已存在时按 `overwrite` 判定，规则含义见 [`FtpOverwrite`]。
///
/// 时间比较容忍 2 秒误差（FAT/FTP 的时间戳精度只有 2 秒），且两侧时间戳都拿得到
/// 才比时间——服务器不支持 `MDTM` 时，「本地更新」这个问题根本无法回答，
/// 此时 `size_or_newer` 与 `newer` 都只能判定为「不确定 → 不传」，
/// 真要传得上去只有 `always`。这一条写进了 docs/deploy.md。
pub fn plan_sync(
    local: &[LocalEntry],
    remote: &[RemoteEntry],
    delete_extra: bool,
    overwrite: FtpOverwrite,
) -> SyncPlan {
    let mut plan = SyncPlan::default();

    for item in local {
        let existing = remote.iter().find(|r| r.path == item.path);
        let upload = match existing {
            None => true,
            Some(r) => should_overwrite(item, r, overwrite),
        };
        if upload {
            plan.upload.push(item.path.clone());
        } else {
            plan.skipped += 1;
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

/// 远端已有同名文件时，这一份要不要重传。
fn should_overwrite(local: &LocalEntry, remote: &RemoteEntry, overwrite: FtpOverwrite) -> bool {
    let size_differs = local.size != remote.size;
    // 两侧都有时间戳才谈得上「更新」；容忍 2 秒是为了 FAT/FTP 的精度。
    let local_is_newer = match (local.modified_epoch, remote.modified_epoch) {
        (Some(l), Some(r)) => l > r + 2,
        _ => false,
    };

    match overwrite {
        FtpOverwrite::Always => true,
        FtpOverwrite::Skip => false,
        FtpOverwrite::Size => size_differs,
        FtpOverwrite::Newer => local_is_newer,
        FtpOverwrite::SizeOrNewer => size_differs || local_is_newer,
    }
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

        let plan = plan_sync(&local_files, &remote_files, false, FtpOverwrite::default());
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
            FtpOverwrite::default(),
        );
        assert_eq!(plan.upload, vec!["a.html"]);
    }

    #[test]
    fn two_second_clock_skew_is_tolerated() {
        let plan = plan_sync(
            &[local("a.html", 10, Some(1_002))],
            &[remote("a.html", 10, Some(1_000))],
            false,
            FtpOverwrite::default(),
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
            FtpOverwrite::default(),
        );
        assert!(plan.upload.is_empty());
    }

    /// 服务器时钟快于本地时，默认规则会把**所有**文件永久跳过。
    ///
    /// 这是 FTP 发布最难发现的失败：界面只显示「跳过 N 个」，看起来像「没有改动」，
    /// 而实际上线上一直是旧的。判定材料只有大小与 MDTM，我们分不清「远端确实更新」
    /// 和「服务器时钟快了一小时」，所以出路是让用户选规则，而不是猜。
    #[test]
    fn server_clock_ahead_silently_skips_unless_the_user_picks_another_rule() {
        // 本地文件刚重新生成（t=1000），远端时间戳却是一小时后（服务器时钟快）。
        let local_files = [local("a.html", 10, Some(1_000))];
        let remote_files = [remote("a.html", 10, Some(4_600))];

        // 默认规则：跳过——这就是那个陷阱，行为保持不变但现在有出路。
        let default_plan = plan_sync(
            &local_files,
            &remote_files,
            false,
            FtpOverwrite::SizeOrNewer,
        );
        assert!(default_plan.upload.is_empty());

        // 「总是上传」：不比对，一定传。
        let always = plan_sync(&local_files, &remote_files, false, FtpOverwrite::Always);
        assert_eq!(always.upload, vec!["a.html"]);
        assert_eq!(always.skipped, 0);
    }

    /// 同长度改动 + 服务器不给 MDTM：默认与 `size` 规则都会跳过，`always` 才传得上去。
    #[test]
    fn same_size_edit_without_remote_timestamp_needs_always() {
        let local_files = [local("a.html", 10, Some(2_000))];
        let remote_files = [remote("a.html", 10, None)];

        for rule in [
            FtpOverwrite::SizeOrNewer,
            FtpOverwrite::Size,
            FtpOverwrite::Newer,
            FtpOverwrite::Skip,
        ] {
            let plan = plan_sync(&local_files, &remote_files, false, rule);
            assert!(plan.upload.is_empty(), "{rule:?} 不该判定为要上传");
            assert_eq!(plan.skipped, 1, "{rule:?}");
        }

        let plan = plan_sync(&local_files, &remote_files, false, FtpOverwrite::Always);
        assert_eq!(plan.upload, vec!["a.html"]);
    }

    /// 每条规则只看它该看的那一项。
    #[test]
    fn each_rule_looks_at_its_own_signal() {
        let bigger = [local("a.html", 20, Some(1_000))];
        let older_remote = [remote("a.html", 10, Some(2_000))];

        // 只看大小：大小不同就传，远端时间更新也不影响。
        assert_eq!(
            plan_sync(&bigger, &older_remote, false, FtpOverwrite::Size).upload,
            vec!["a.html"]
        );
        // 只看时间：远端更新 → 跳过，哪怕大小不同。
        assert!(
            plan_sync(&bigger, &older_remote, false, FtpOverwrite::Newer)
                .upload
                .is_empty()
        );

        let newer_local = [local("a.html", 10, Some(3_000))];
        let remote_same_size = [remote("a.html", 10, Some(1_000))];
        // 只看大小：大小一样就跳过，哪怕本地更新。
        assert!(
            plan_sync(&newer_local, &remote_same_size, false, FtpOverwrite::Size)
                .upload
                .is_empty()
        );
        assert_eq!(
            plan_sync(&newer_local, &remote_same_size, false, FtpOverwrite::Newer).upload,
            vec!["a.html"]
        );
    }

    /// 任何规则都不影响「远端没有这个文件」——新文件一律要传，跳过规则也不例外。
    #[test]
    fn missing_remote_files_are_always_uploaded() {
        let local_files = [local("new.html", 10, Some(1_000))];
        for rule in [
            FtpOverwrite::SizeOrNewer,
            FtpOverwrite::Always,
            FtpOverwrite::Newer,
            FtpOverwrite::Size,
            FtpOverwrite::Skip,
        ] {
            let plan = plan_sync(&local_files, &[], false, rule);
            assert_eq!(plan.upload, vec!["new.html"], "{rule:?}");
        }
    }

    /// `skip`：远端已存在就不动，只补新文件。
    #[test]
    fn skip_rule_leaves_existing_remote_files_alone() {
        let local_files = [
            local("a.html", 999, Some(9_999)),
            local("new.html", 10, Some(1_000)),
        ];
        let remote_files = [remote("a.html", 10, Some(1_000))];

        let plan = plan_sync(&local_files, &remote_files, false, FtpOverwrite::Skip);
        assert_eq!(plan.upload, vec!["new.html"]);
        assert_eq!(plan.skipped, 1);
    }

    #[test]
    fn delete_extra_is_opt_in() {
        let local_files = vec![local("a.html", 1, None)];
        let remote_files = vec![remote("a.html", 1, None), remote("stale.html", 1, None)];

        assert!(
            plan_sync(&local_files, &remote_files, false, FtpOverwrite::default())
                .delete
                .is_empty()
        );
        assert_eq!(
            plan_sync(&local_files, &remote_files, true, FtpOverwrite::default()).delete,
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
