use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{Error, Result};

/// 本地 SQLite 索引：保存页面/模板哈希、依赖关系与脏标记，是增量构建的依据。
///
/// 索引可以被安全删除，下一次构建会自动重建（等价于全量生成）。
pub struct Index {
    conn: Connection,
}

/// 页面在索引中的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageRecord {
    pub source: String,
    pub output: String,
    pub url: String,
    pub title: String,
    pub template: String,
    /// 源文件哈希。
    pub hash: String,
    /// 该页面上次渲染时所用模板集合的组合哈希，用于识别「模板变了但内容没变」。
    pub template_hash: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildRecord {
    pub id: i64,
    pub finished_at: String,
    pub mode: String,
    pub pages_written: usize,
    pub duration_ms: u64,
}

impl Index {
    /// 打开（或创建）索引数据库。父目录会被自动创建。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let conn = Connection::open(path)?;
        let index = Self { conn };
        index.migrate()?;
        Ok(index)
    }

    /// 内存索引，用于测试与「一次性全量构建」场景。
    pub fn in_memory() -> Result<Self> {
        let index = Self {
            conn: Connection::open_in_memory()?,
        };
        index.migrate()?;
        Ok(index)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS pages (
                source        TEXT PRIMARY KEY,
                output        TEXT NOT NULL,
                url           TEXT NOT NULL,
                title         TEXT NOT NULL,
                template      TEXT NOT NULL,
                hash          TEXT NOT NULL,
                template_hash TEXT NOT NULL DEFAULT '',
                dirty         INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_pages_template ON pages(template);
            CREATE INDEX IF NOT EXISTS idx_pages_dirty ON pages(dirty);

            CREATE TABLE IF NOT EXISTS templates (
                name TEXT PRIMARY KEY,
                hash TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'partial'
            );

            CREATE TABLE IF NOT EXISTS template_deps (
                template   TEXT NOT NULL,
                depends_on TEXT NOT NULL,
                PRIMARY KEY (template, depends_on)
            );

            CREATE TABLE IF NOT EXISTS builds (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                finished_at   TEXT NOT NULL,
                mode          TEXT NOT NULL,
                pages_written INTEGER NOT NULL,
                duration_ms   INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_page(&self, record: &PageRecord) -> Result<()> {
        self.conn.execute(
            r#"
            INSERT INTO pages (source, output, url, title, template, hash, template_hash, dirty)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(source) DO UPDATE SET
                output = excluded.output,
                url = excluded.url,
                title = excluded.title,
                template = excluded.template,
                hash = excluded.hash,
                template_hash = excluded.template_hash,
                dirty = excluded.dirty
            "#,
            params![
                record.source,
                record.output,
                record.url,
                record.title,
                record.template,
                record.hash,
                record.template_hash,
                record.dirty as i32,
            ],
        )?;
        Ok(())
    }

    pub fn page(&self, source: &str) -> Result<Option<PageRecord>> {
        let record = self
            .conn
            .query_row(
                "SELECT source, output, url, title, template, hash, template_hash, dirty \
                 FROM pages WHERE source = ?1",
                params![source],
                row_to_page,
            )
            .optional()?;
        Ok(record)
    }

    pub fn pages(&self) -> Result<Vec<PageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, output, url, title, template, hash, template_hash, dirty \
             FROM pages ORDER BY source",
        )?;
        let rows = stmt.query_map([], row_to_page)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 把指定页面标记为待重新生成。
    pub fn mark_dirty(&self, sources: &[String]) -> Result<usize> {
        let mut changed = 0;
        for source in sources {
            changed += self.conn.execute(
                "UPDATE pages SET dirty = 1 WHERE source = ?1",
                params![source],
            )?;
        }
        Ok(changed)
    }

    /// 按模板名标记脏页面：级联更新的核心操作。
    pub fn mark_dirty_by_templates(&self, templates: &[String]) -> Result<usize> {
        let mut changed = 0;
        for template in templates {
            changed += self.conn.execute(
                "UPDATE pages SET dirty = 1 WHERE template = ?1",
                params![template],
            )?;
        }
        Ok(changed)
    }

    pub fn clear_dirty(&self, sources: &[String]) -> Result<()> {
        for source in sources {
            self.conn.execute(
                "UPDATE pages SET dirty = 0 WHERE source = ?1",
                params![source],
            )?;
        }
        Ok(())
    }

    pub fn dirty_pages(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source FROM pages WHERE dirty = 1 ORDER BY source")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 删除索引中不再存在于磁盘的页面，返回被删除的记录（调用方据此清理 `dist/` 产物）。
    pub fn prune_pages(&self, existing: &[String]) -> Result<Vec<PageRecord>> {
        let removed: Vec<PageRecord> = self
            .pages()?
            .into_iter()
            .filter(|p| !existing.contains(&p.source))
            .collect();
        for page in &removed {
            self.conn
                .execute("DELETE FROM pages WHERE source = ?1", params![page.source])?;
        }
        Ok(removed)
    }

    pub fn template_hashes(&self) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT name, hash FROM templates")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
    }

    /// 用当前磁盘状态整体替换模板哈希与依赖表。
    pub fn replace_templates(
        &mut self,
        hashes: &BTreeMap<String, String>,
        deps: &BTreeMap<String, Vec<String>>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM templates", [])?;
        tx.execute("DELETE FROM template_deps", [])?;
        for (name, hash) in hashes {
            tx.execute(
                "INSERT INTO templates (name, hash) VALUES (?1, ?2)",
                params![name, hash],
            )?;
        }
        for (template, list) in deps {
            for dep in list {
                tx.execute(
                    "INSERT OR IGNORE INTO template_deps (template, depends_on) VALUES (?1, ?2)",
                    params![template, dep],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_build(&self, mode: &str, pages_written: usize, duration_ms: u64) -> Result<i64> {
        let finished_at = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO builds (finished_at, mode, pages_written, duration_ms) \
             VALUES (?1, ?2, ?3, ?4)",
            params![finished_at, mode, pages_written as i64, duration_ms as i64],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn recent_builds(&self, limit: usize) -> Result<Vec<BuildRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, finished_at, mode, pages_written, duration_ms \
             FROM builds ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok(BuildRecord {
                id: r.get(0)?,
                finished_at: r.get(1)?,
                mode: r.get(2)?,
                pages_written: r.get::<_, i64>(3)? as usize,
                duration_ms: r.get::<_, i64>(4)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn row_to_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<PageRecord> {
    Ok(PageRecord {
        source: row.get(0)?,
        output: row.get(1)?,
        url: row.get(2)?,
        title: row.get(3)?,
        template: row.get(4)?,
        hash: row.get(5)?,
        template_hash: row.get(6)?,
        dirty: row.get::<_, i32>(7)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(source: &str, template: &str) -> PageRecord {
        PageRecord {
            source: source.to_string(),
            output: format!("{source}.html"),
            url: format!("/{source}/"),
            title: source.to_string(),
            template: template.to_string(),
            hash: "h1".to_string(),
            template_hash: "t1".to_string(),
            dirty: false,
        }
    }

    #[test]
    fn upsert_is_idempotent_and_updates_fields() {
        let index = Index::in_memory().unwrap();
        index
            .upsert_page(&record("a.md", "pages/post.html"))
            .unwrap();
        let mut updated = record("a.md", "pages/post.html");
        updated.title = "新标题".into();
        index.upsert_page(&updated).unwrap();
        assert_eq!(index.pages().unwrap().len(), 1);
        assert_eq!(index.page("a.md").unwrap().unwrap().title, "新标题");
    }

    #[test]
    fn mark_dirty_by_template_selects_matching_pages() {
        let index = Index::in_memory().unwrap();
        index
            .upsert_page(&record("a.md", "pages/post.html"))
            .unwrap();
        index
            .upsert_page(&record("b.md", "pages/index.html"))
            .unwrap();

        let n = index
            .mark_dirty_by_templates(&["pages/post.html".to_string()])
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(index.dirty_pages().unwrap(), vec!["a.md"]);

        index.clear_dirty(&["a.md".to_string()]).unwrap();
        assert!(index.dirty_pages().unwrap().is_empty());
    }

    #[test]
    fn prune_returns_removed_records() {
        let index = Index::in_memory().unwrap();
        index
            .upsert_page(&record("a.md", "pages/post.html"))
            .unwrap();
        index
            .upsert_page(&record("b.md", "pages/post.html"))
            .unwrap();
        let removed = index.prune_pages(&["a.md".to_string()]).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].source, "b.md");
        assert_eq!(index.pages().unwrap().len(), 1);
    }

    #[test]
    fn template_hashes_round_trip() {
        let mut index = Index::in_memory().unwrap();
        let hashes = BTreeMap::from([
            ("layouts/base.html".to_string(), "h1".to_string()),
            ("components/header.html".to_string(), "h2".to_string()),
        ]);
        let deps = BTreeMap::from([(
            "layouts/base.html".to_string(),
            vec!["components/header.html".to_string()],
        )]);
        index.replace_templates(&hashes, &deps).unwrap();
        assert_eq!(index.template_hashes().unwrap(), hashes);

        // 再次替换应完全覆盖旧数据，而不是累加。
        index
            .replace_templates(&BTreeMap::new(), &BTreeMap::new())
            .unwrap();
        assert!(index.template_hashes().unwrap().is_empty());
    }

    #[test]
    fn build_history_is_recorded_newest_first() {
        let index = Index::in_memory().unwrap();
        index.record_build("full", 10, 120).unwrap();
        index.record_build("incremental", 2, 8).unwrap();
        let builds = index.recent_builds(5).unwrap();
        assert_eq!(builds.len(), 2);
        assert_eq!(builds[0].mode, "incremental");
        assert_eq!(builds[0].pages_written, 2);
    }
}
