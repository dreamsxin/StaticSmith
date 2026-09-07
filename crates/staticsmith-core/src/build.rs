use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;
use tera::Context;

use crate::config::{ProjectPaths, SiteConfig};
use crate::content::{self, Page};
use crate::error::{Error, Result};
use crate::index::{Index, PageRecord};
use crate::templates::TemplateSet;
use crate::util;

/// 生成策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildMode {
    /// 全量生成：重渲染全部页面。
    Full,
    /// 智能增量：只渲染内容变化的页面 + 受变更组件影响的页面。
    Incremental,
}

impl BuildMode {
    fn as_str(self) -> &'static str {
        match self {
            BuildMode::Full => "full",
            BuildMode::Incremental => "incremental",
        }
    }
}

/// 构建计划：点击「生成」前用于向用户展示影响范围
/// （「检测到全局组件变更，影响 32 个页面，是否立即重新生成？」）。
#[derive(Debug, Clone, Serialize)]
pub struct BuildPlan {
    pub mode: BuildMode,
    /// 待渲染的内容源路径。
    pub pages: Vec<String>,
    /// 内容发生变化的模板。
    pub changed_templates: Vec<String>,
    /// 变更传递闭包内的全部模板。
    pub affected_templates: Vec<String>,
    /// 站点当前可发布页面总数。
    pub total_pages: usize,
    /// 索引中存在但内容目录已删除的页面，其产物将被清理。
    pub orphaned_pages: Vec<String>,
}

impl BuildPlan {
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty() && self.orphaned_pages.is_empty()
    }
}

/// 构建结果。
#[derive(Debug, Clone, Serialize)]
pub struct BuildReport {
    pub mode: BuildMode,
    /// 参与渲染的内容页数量。
    pub pages_rendered: usize,
    /// 实际写盘的 HTML 文件数量（分页会让它大于 `pages_rendered`）。
    pub files_written: usize,
    pub assets_copied: usize,
    pub removed_files: Vec<String>,
    pub duration_ms: u64,
    /// 非致命问题，例如缺少静态资源目录。
    pub warnings: Vec<String>,
}

/// 静态生成引擎。持有配置、模板集合与本地索引。
pub struct Builder {
    pub config: SiteConfig,
    pub paths: ProjectPaths,
    templates: TemplateSet,
    index: Index,
    pages: Vec<Page>,
}

impl Builder {
    /// 打开项目：读取配置、模板与内容，并连接本地索引。
    pub fn open(project_root: impl AsRef<Path>) -> Result<Self> {
        let root = project_root.as_ref();
        let config = SiteConfig::load(root)?;
        let issues = config.validate();
        if !issues.is_empty() {
            return Err(Error::InvalidProject(issues.join("; ")));
        }
        let paths = ProjectPaths::new(root, &config.build);
        let templates = TemplateSet::load(&paths.templates)?;
        let index = Index::open(&paths.index_db)?;
        let pages = content::load_all(&paths.content)?;
        Ok(Self {
            config,
            paths,
            templates,
            index,
            pages,
        })
    }

    /// 重新读取模板与内容。文件监听触发变更后调用。
    pub fn reload(&mut self) -> Result<()> {
        self.templates = TemplateSet::load(&self.paths.templates)?;
        self.pages = content::load_all(&self.paths.content)?;
        Ok(())
    }

    pub fn templates(&self) -> &TemplateSet {
        &self.templates
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// 计算构建计划，不写任何文件。
    pub fn plan(&self, mode: BuildMode) -> Result<BuildPlan> {
        let known = self.index.template_hashes()?;
        let changed_templates = self.templates.changed_since(&known);
        let affected: BTreeSet<String> = self
            .templates
            .graph
            .affected_templates(changed_templates.iter().map(String::as_str));

        let publishable: Vec<&Page> = self.pages.iter().filter(|p| p.is_publishable()).collect();
        let existing: Vec<String> = publishable.iter().map(|p| p.source.clone()).collect();
        let orphaned_pages: Vec<String> = self
            .index
            .pages()?
            .into_iter()
            .filter(|r| !existing.contains(&r.source))
            .map(|r| r.source)
            .collect();

        let pages = match mode {
            BuildMode::Full => existing.clone(),
            BuildMode::Incremental => {
                let mut selected = Vec::new();
                for page in &publishable {
                    if self.needs_rebuild(page, &affected)? {
                        selected.push(page.source.clone());
                    }
                }
                selected
            }
        };

        Ok(BuildPlan {
            mode,
            pages,
            changed_templates,
            affected_templates: affected.into_iter().collect(),
            total_pages: publishable.len(),
            orphaned_pages,
        })
    }

    /// 单个页面是否需要重新渲染。
    fn needs_rebuild(&self, page: &Page, affected_templates: &BTreeSet<String>) -> Result<bool> {
        let Some(record) = self.index.page(&page.source)? else {
            return Ok(true); // 从未构建过
        };
        if record.dirty || record.hash != page.hash {
            return Ok(true); // 内容变化或被显式标脏
        }
        if affected_templates.contains(&page.template) {
            return Ok(true); // 级联更新：所用模板或其依赖的组件发生变化
        }
        if !self.paths.output.join(&page.output).exists() {
            return Ok(true); // 产物被手动删除
        }
        Ok(false)
    }

    /// 执行构建。渲染阶段使用 Rayon 多核并行。
    pub fn build(&mut self, mode: BuildMode) -> Result<BuildReport> {
        let started = Instant::now();
        let plan = self.plan(mode)?;
        let mut warnings = Vec::new();

        let selected: Vec<&Page> = self
            .pages
            .iter()
            .filter(|p| plan.pages.contains(&p.source))
            .collect();

        let site_ctx = self.site_context();
        let all_pages: Vec<&Page> = self.pages.iter().filter(|p| p.is_publishable()).collect();
        let renderer = Renderer {
            templates: &self.templates,
            config: &self.config,
        };

        // 并行渲染：每个页面产出一个或多个（分页）HTML 文件。
        let rendered: Vec<Result<RenderedPage>> = selected
            .par_iter()
            .map(|page| renderer.render_page(page, &site_ctx, &all_pages))
            .collect();

        let mut outputs = Vec::new();
        for item in rendered {
            outputs.push(item?);
        }

        let mut files_written = 0;
        for output in &outputs {
            for file in &output.files {
                self.write_output(&file.path, &file.html)?;
                files_written += 1;
            }
        }

        // 清理已删除内容的产物。
        let mut removed_files = Vec::new();
        for record in self.index.prune_pages(
            &all_pages
                .iter()
                .map(|p| p.source.clone())
                .collect::<Vec<_>>(),
        )? {
            let path = self.paths.output.join(&record.output);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
                removed_files.push(record.output);
            }
        }

        let assets_copied = match self.copy_assets() {
            Ok(n) => n,
            Err(e) => {
                warnings.push(format!("静态资源复制失败: {e}"));
                0
            }
        };

        // 索引更新：写入新哈希并清除脏标记。
        let template_hash = self.combined_template_hash();
        for page in &outputs {
            self.index.upsert_page(&PageRecord {
                source: page.source.clone(),
                output: page.primary_output.clone(),
                url: page.url.clone(),
                title: page.title.clone(),
                template: page.template.clone(),
                hash: page.hash.clone(),
                template_hash: template_hash.clone(),
                dirty: false,
            })?;
        }
        let hashes = self.templates.hashes();
        let deps: BTreeMap<String, Vec<String>> = self
            .templates
            .infos()
            .map(|i| (i.name.clone(), i.dependencies.clone()))
            .collect();
        self.index.replace_templates(&hashes, &deps)?;

        let duration_ms = started.elapsed().as_millis() as u64;
        self.index
            .record_build(mode.as_str(), outputs.len(), duration_ms)?;

        Ok(BuildReport {
            mode,
            pages_rendered: outputs.len(),
            files_written,
            assets_copied,
            removed_files,
            duration_ms,
            warnings,
        })
    }

    /// 渲染单个内容页在其父级布局下的最终 HTML（供编辑器实时预览使用，不写盘）。
    pub fn preview(&self, source: &str) -> Result<String> {
        let page = self
            .pages
            .iter()
            .find(|p| p.source == source)
            .ok_or_else(|| Error::Other(format!("内容不存在: {source}")))?;
        let all_pages: Vec<&Page> = self.pages.iter().filter(|p| p.is_publishable()).collect();
        let renderer = Renderer {
            templates: &self.templates,
            config: &self.config,
        };
        let rendered = renderer.render_page(page, &self.site_context(), &all_pages)?;
        rendered
            .files
            .into_iter()
            .next()
            .map(|f| f.html)
            .ok_or_else(|| Error::Other("渲染结果为空".to_string()))
    }

    fn site_context(&self) -> Context {
        let mut ctx = Context::new();
        ctx.insert("site", &self.config.site);
        ctx.insert("build", &self.config.build);
        ctx.insert("generator", "StaticSmith 2.0");
        ctx
    }

    fn write_output(&self, relative: &str, html: &str) -> Result<()> {
        let path = self.paths.output.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        std::fs::write(&path, html).map_err(|e| Error::io(&path, e))
    }

    /// 复制 `themes/<theme>/static/` 与项目根 `static/` 到输出目录。
    fn copy_assets(&self) -> Result<usize> {
        let mut copied = 0;
        for dir in [
            self.paths.theme.join("static"),
            self.paths.root.join("static"),
        ] {
            if dir.exists() {
                copied += copy_dir(&dir, &self.paths.output)?;
            }
        }
        Ok(copied)
    }

    /// 全部模板哈希的聚合值，用于判断「模板整体是否变化」。
    fn combined_template_hash(&self) -> String {
        let joined = self
            .templates
            .hashes()
            .into_iter()
            .map(|(n, h)| format!("{n}:{h}"))
            .collect::<Vec<_>>()
            .join("|");
        util::hash_str(&joined)
    }
}

/// 无状态渲染器。
///
/// 单独拆出来是为了让并行渲染只借用模板与配置——`Builder` 持有的 SQLite 连接不是 `Sync`。
struct Renderer<'a> {
    templates: &'a TemplateSet,
    config: &'a SiteConfig,
}

impl Renderer<'_> {
    fn render_page(
        &self,
        page: &Page,
        site_ctx: &Context,
        all_pages: &[&Page],
    ) -> Result<RenderedPage> {
        let mut files = Vec::new();

        if page.is_index {
            let items = section_items(page, all_pages);
            let per_page = self.config.build.page_size.max(1);
            let total_pages = items.len().div_ceil(per_page).max(1);

            for current in 1..=total_pages {
                let start = (current - 1) * per_page;
                let slice: Vec<&Page> = items.iter().skip(start).take(per_page).copied().collect();
                let pagination = Pagination::new(&page.url, current, total_pages);

                let mut ctx = site_ctx.clone();
                ctx.insert("page", page);
                ctx.insert("pages", all_pages);
                ctx.insert("items", &slice);
                ctx.insert("pagination", &pagination);

                let html = self.finish(self.templates.tera.render(&page.template, &ctx)?);
                files.push(RenderedFile {
                    path: pagination.output_path(&page.output),
                    html,
                });
            }
        } else {
            let mut ctx = site_ctx.clone();
            ctx.insert("page", page);
            ctx.insert("pages", all_pages);
            let html = self.finish(self.templates.tera.render(&page.template, &ctx)?);
            files.push(RenderedFile {
                path: page.output.clone(),
                html,
            });
        }

        Ok(RenderedPage {
            source: page.source.clone(),
            primary_output: page.output.clone(),
            url: page.url.clone(),
            title: page.title.clone(),
            template: page.template.clone(),
            hash: page.hash.clone(),
            files,
        })
    }

    fn finish(&self, html: String) -> String {
        if self.config.build.minify {
            util::minify_html(&html)
        } else {
            html
        }
    }
}

struct RenderedPage {
    source: String,
    primary_output: String,
    url: String,
    title: String,
    template: String,
    hash: String,
    files: Vec<RenderedFile>,
}

struct RenderedFile {
    path: String,
    html: String,
}

/// 传给 `components/pagination.html` 的上下文。
#[derive(Debug, Clone, Serialize)]
pub struct Pagination {
    pub current_page: usize,
    pub total_pages: usize,
    /// 分页所属栏目的地址，如 `/posts/`。
    pub base_url: String,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    /// 每一页的地址，供模板渲染页码条。
    pub page_urls: Vec<String>,
}

impl Pagination {
    pub fn new(base_url: &str, current_page: usize, total_pages: usize) -> Self {
        let url_for = |n: usize| {
            if n <= 1 {
                base_url.to_string()
            } else {
                format!("{}page/{}/", base_url, n)
            }
        };
        Self {
            prev_url: (current_page > 1).then(|| url_for(current_page - 1)),
            next_url: (current_page < total_pages).then(|| url_for(current_page + 1)),
            page_urls: (1..=total_pages).map(url_for).collect(),
            base_url: base_url.to_string(),
            current_page,
            total_pages,
        }
    }

    /// 第 1 页写在栏目根，其余写到 `page/N/index.html`。
    fn output_path(&self, primary_output: &str) -> String {
        if self.current_page <= 1 {
            return primary_output.to_string();
        }
        let dir = primary_output
            .trim_end_matches("index.html")
            .trim_end_matches('/');
        if dir.is_empty() {
            format!("page/{}/index.html", self.current_page)
        } else {
            format!("{}/page/{}/index.html", dir, self.current_page)
        }
    }
}

/// 栏目索引页所列出的条目：同目录下的非索引页，按 weight 升序、日期降序排列。
fn section_items<'a>(index_page: &Page, all_pages: &[&'a Page]) -> Vec<&'a Page> {
    let mut items: Vec<&Page> = all_pages
        .iter()
        .filter(|p| !p.is_index && p.section == index_page.section)
        .copied()
        .collect();
    items.sort_by(|a, b| {
        a.weight
            .cmp(&b.weight)
            .then_with(|| b.date.cmp(&a.date))
            .then_with(|| a.title.cmp(&b.title))
    });
    items
}

/// 递归复制目录内容（覆盖同名文件），返回复制的文件数。
fn copy_dir(from: &Path, to: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let rel = entry.path().strip_prefix(from).unwrap_or(entry.path());
        let target: PathBuf = to.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        std::fs::copy(entry.path(), &target).map_err(|e| Error::io(&target, e))?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_first_page_writes_section_root() {
        let p = Pagination::new("/posts/", 1, 3);
        assert_eq!(p.output_path("posts/index.html"), "posts/index.html");
        assert_eq!(p.prev_url, None);
        assert_eq!(p.next_url.as_deref(), Some("/posts/page/2/"));
    }

    #[test]
    fn pagination_second_page_writes_subdirectory() {
        let p = Pagination::new("/posts/", 2, 3);
        assert_eq!(p.output_path("posts/index.html"), "posts/page/2/index.html");
        assert_eq!(p.prev_url.as_deref(), Some("/posts/"));
        assert_eq!(p.next_url.as_deref(), Some("/posts/page/3/"));
    }

    #[test]
    fn pagination_handles_site_root() {
        let p = Pagination::new("/", 2, 2);
        assert_eq!(p.output_path("index.html"), "page/2/index.html");
        assert_eq!(p.page_urls, vec!["/", "/page/2/"]);
    }
}
