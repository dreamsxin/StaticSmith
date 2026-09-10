//! 最近打开的站点。
//!
//! 这是第一份**应用级**状态：它不属于任何站点，因此不能放进 `staticsmith.toml`
//! 或 `.staticsmith/index.db`，而是存到操作系统的应用配置目录。
//!
//! 读取失败一律降级为空列表——最近列表是便利功能，不该阻断应用启动。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, Result};

/// 配置文件名，位于 `app_config_dir()` 下。
const FILE_NAME: &str = "recent.json";

/// 一条最近记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// 打开时的站点标题，用于列表显示（改标题后下次打开会刷新）。
    pub title: String,
    /// RFC3339 时间戳。
    pub opened_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentProjects {
    #[serde(default)]
    pub items: Vec<RecentEntry>,
}

impl RecentProjects {
    /// 列表上限。超过就丢掉最旧的——这是「最近」列表，不是历史归档。
    pub const MAX: usize = 10;

    /// 记录一次打开：同路径去重后置顶。
    pub fn touch(&mut self, path: &Path, title: &str, opened_at: String) {
        let path = normalize(path);
        self.items.retain(|item| normalize(&item.path) != path);
        self.items.insert(
            0,
            RecentEntry {
                path,
                title: title.to_string(),
                opened_at,
            },
        );
        self.items.truncate(Self::MAX);
    }

    pub fn remove(&mut self, path: &Path) {
        let path = normalize(path);
        self.items.retain(|item| normalize(&item.path) != path);
    }

    /// 丢掉已经不存在（被删除或移动）的站点，返回被清理的条数。
    pub fn prune_missing(&mut self) -> usize {
        let before = self.items.len();
        self.items
            .retain(|item| item.path.join(staticsmith_core::CONFIG_FILE_NAME).is_file());
        before - self.items.len()
    }
}

/// 读取最近列表并顺手清理失效条目。任何错误都降级为空列表。
pub fn load(app: &AppHandle) -> RecentProjects {
    let Ok(path) = file_path(app) else {
        return RecentProjects::default();
    };
    let mut list = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<RecentProjects>(&raw).ok())
        .unwrap_or_default();
    if list.prune_missing() > 0 {
        // 清理结果写不回去就记一声，别静默：不然「删掉的站点反复出现在最近列表里」
        // 会被当成界面 bug 去查，而真正的原因在磁盘上。策略与 `record` 保持一致。
        if let Err(err) = save(app, &list) {
            tracing::warn!("清理后的最近列表写盘失败，下次打开还会看到失效条目: {err}");
        }
    }
    list
}

pub fn save(app: &AppHandle, list: &RecentProjects) -> Result<()> {
    let path = file_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(list)
        .map_err(|e| AppError::Message(format!("最近列表序列化失败: {e}")))?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// 记录一次成功打开。写盘失败只记日志：不该因为写不了便利文件而让「打开项目」失败。
pub fn record(app: &AppHandle, root: &Path, title: &str) {
    let mut list = load(app);
    list.touch(root, title, chrono_now());
    if let Err(err) = save(app, &list) {
        tracing::warn!("最近列表写入失败: {err}");
    }
}

fn file_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Message(format!("取应用配置目录失败: {e}")))?;
    Ok(dir.join(FILE_NAME))
}

/// 规范化路径用于去重。
///
/// Windows 上 `canonicalize` 会带 `\\?\` 前缀，直接显示给用户很难看，这里剥掉。
pub fn normalize(path: &Path) -> PathBuf {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => canonical,
    }
}

fn chrono_now() -> String {
    chrono::Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_paths(list: &RecentProjects) -> Vec<String> {
        list.items
            .iter()
            .map(|i| i.path.to_string_lossy().to_string())
            .collect()
    }

    /// 一个**确定不存在**的路径。
    ///
    /// 不能写死 `/a` 这种字面量：Windows 上它是「当前盘根目录」相对路径，
    /// 在 CI runner（工作盘 `D:`，工作目录就是 `D:\a`）上真的存在，
    /// `normalize` 里的 `canonicalize` 会成功并返回 `D:\a`，断言随之对不上。
    /// 挂在临时目录下的子路径在任何平台都不存在。
    fn missing_path(dir: &tempfile::TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    #[test]
    fn touch_moves_existing_entries_to_the_front() {
        // 用真实存在的目录：normalize 会 canonicalize，走成功分支才是实际运行时的样子
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let mut list = RecentProjects::default();
        list.touch(a.path(), "A", "t1".into());
        list.touch(b.path(), "B", "t2".into());
        list.touch(a.path(), "A 改名", "t3".into());

        assert_eq!(list.items.len(), 2, "同路径不应重复");
        assert_eq!(list.items[0].title, "A 改名", "标题以最后一次为准");
        assert_eq!(list.items[0].path, normalize(a.path()));
    }

    #[test]
    fn touch_keeps_only_the_most_recent_entries() {
        let mut list = RecentProjects::default();
        for i in 0..(RecentProjects::MAX + 5) {
            list.touch(Path::new(&format!("/site-{i}")), "t", "now".into());
        }
        assert_eq!(list.items.len(), RecentProjects::MAX);
        assert!(entry_paths(&list)[0].contains(&format!("site-{}", RecentProjects::MAX + 4)));
    }

    #[test]
    fn remove_drops_the_entry() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let mut list = RecentProjects::default();
        list.touch(a.path(), "A", "t".into());
        list.touch(b.path(), "B", "t".into());
        list.remove(a.path());
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].path, normalize(b.path()));
    }

    #[test]
    fn prune_drops_directories_that_are_no_longer_projects() {
        let dir = tempfile::tempdir().unwrap();
        staticsmith_core::scaffold::init_project(
            dir.path(),
            Some("在的"),
            staticsmith_core::scaffold::Preset::Docs,
        )
        .unwrap();

        let mut list = RecentProjects::default();
        list.touch(dir.path(), "在的", "t".into());
        list.touch(&missing_path(&dir, "gone"), "没了", "t".into());

        assert_eq!(list.prune_missing(), 1);
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].title, "在的");
    }

    #[test]
    fn normalize_strips_the_windows_verbatim_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let normalized = normalize(dir.path());
        assert!(
            !normalized.to_string_lossy().starts_with(r"\\?\"),
            "路径要能直接显示给用户: {}",
            normalized.display()
        );
    }

    #[test]
    fn normalize_keeps_nonexistent_paths_as_given() {
        let dir = tempfile::tempdir().unwrap();
        let path = missing_path(&dir, "no-such-child");
        assert_eq!(normalize(&path), path);
    }

    #[test]
    fn json_round_trip_preserves_entries() {
        let mut list = RecentProjects::default();
        list.touch(
            Path::new("/a"),
            "站点 A",
            "2026-09-07T00:00:00+08:00".into(),
        );

        let raw = serde_json::to_string(&list).unwrap();
        let parsed: RecentProjects = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.items, list.items);
    }

    #[test]
    fn missing_or_broken_file_yields_an_empty_list() {
        let parsed: RecentProjects = serde_json::from_str("{}").unwrap();
        assert!(parsed.items.is_empty());
        assert!(serde_json::from_str::<RecentProjects>("not json").is_err());
    }
}
