use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use staticsmith_core::batch::{
    Action as BatchAction, Moved as BatchMoved, Preview as BatchPreview, Skipped as BatchSkipped,
    TagEdit,
};
use staticsmith_core::build::{BuildMode, BuildPlan, BuildReport};
use staticsmith_core::content::FrontMatter;
use staticsmith_core::graph::TemplateNode;
use staticsmith_core::import::{Candidate as ImportCandidate, Report as ImportReport};
use staticsmith_core::index::{AssetRecord, BuildRecord};
use staticsmith_core::links::Report as LinkReport;
use staticsmith_core::media::{Removed as MediaRemoved, Report as MediaReport};
use staticsmith_core::preview::Inlined as PreviewInlined;
use staticsmith_core::replace::{
    Report as ReplaceResult, Rule as ReplaceRule, Scope as ReplaceScope,
};
use staticsmith_core::search::Hit as SearchHit;
use staticsmith_core::sections::{
    Created as SectionCreated, Meta as SectionMeta, Renamed as SectionRenamed, Section,
};
use staticsmith_core::templates::TemplateInfo;
use staticsmith_core::theme::{
    Exported as ThemeExported, Imported as ThemeImported, Manifest as ThemeManifest,
    Preview as ThemePreview,
};
use staticsmith_core::{
    content, frontmatter, scaffold, templates, NewContent, OutputFile, PreviewServer, SavedAsset,
    SeoReport, SiteConfig,
};
use staticsmith_deploy::{Credentials, DeployReport, Progress};
use staticsmith_mcp::Permissions as McpPermissions;
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::error::{AppError, Result};
use crate::recent;
use crate::state::{AppState, Session, EVENT_BUILD_PROGRESS, EVENT_DEPLOY_PROGRESS};

/// 系统凭据管理器中的服务名。
const KEYRING_SERVICE: &str = "StaticSmith";

/// 打开项目后返回给前端的全量快照。
#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub root: PathBuf,
    pub config: SiteConfig,
    pub pages: Vec<PageSummary>,
    pub layouts: Vec<TemplateInfo>,
    pub components: Vec<TemplateInfo>,
    pub templates: Vec<TemplateInfo>,
    pub recent_builds: Vec<BuildRecord>,
}

/// 页面列表项（不含正文，避免一次性传输整站内容）。
#[derive(Debug, Serialize)]
pub struct PageSummary {
    pub source: String,
    pub title: String,
    pub url: String,
    pub template: String,
    pub section: String,
    pub is_index: bool,
    pub draft: bool,
    pub date: Option<String>,
    /// 非草稿但这次不会进产物：`date` 还没到，且站点关掉了 `publish_future`
    pub scheduled: bool,
    /// 属性面板的标签建议来自这里，因此列表项也要带上
    pub tags: Vec<String>,
}

/// 构建进度事件载荷。
#[derive(Debug, Clone, Serialize)]
pub struct BuildProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct SaveContentArgs {
    pub source: String,
    pub raw: String,
}

/// 栏目改名的参数。
///
/// 用结构体而不是三个独立参数：`keep_aliases` 这种多词参数走结构体最不容易在
/// 前后端命名约定上出岔子，也留了「省略即保留旧地址」的余地。
#[derive(Debug, Deserialize)]
pub struct RenameSectionArgs {
    pub from: String,
    pub to: String,
    /// 省略时按 true：整理结构不该顺手打断所有外部链接。
    #[serde(default = "keep_aliases_default")]
    pub keep_aliases: bool,
}

fn keep_aliases_default() -> bool {
    true
}

/// 栏目元信息的参数。
#[derive(Debug, Deserialize)]
pub struct SectionMetaArgs {
    pub path: String,
    #[serde(flatten)]
    pub meta: SectionMeta,
}

/// 打包主题的参数。
#[derive(Debug, Deserialize)]
pub struct ThemeExportArgs {
    /// 要写出的 zip 路径（由界面的「另存为」对话框给出）。
    pub archive: String,
    #[serde(flatten)]
    pub manifest: ThemeManifest,
}

// ---------------------------------------------------------------- 项目生命周期

/// 新建项目：写入模板、主题与示例内容，返回创建的文件列表。
#[tauri::command]
pub fn init_project(path: PathBuf, title: Option<String>) -> Result<Vec<PathBuf>> {
    let report = scaffold::init_project(&path, title.as_deref())?;
    Ok(report.created)
}

/// 判断目录是否已是 StaticSmith 项目，供「打开」对话框校验。
#[tauri::command]
pub fn is_project(path: PathBuf) -> bool {
    scaffold::is_project(path)
}

#[tauri::command]
pub fn open_project(
    window: Window,
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<ProjectSummary> {
    state.open(&window, &path)?;
    let summary = project_summary(state)?;
    // 打开成功才记入最近列表，避免把打不开的目录留在起始页。
    recent::record(window.app_handle(), &path, &summary.config.site.title);
    Ok(summary)
}

/// 最近打开的站点，最新在前。已被删除或移动的条目会在读取时自动清理。
#[tauri::command]
pub fn recent_projects(app: AppHandle) -> Vec<recent::RecentEntry> {
    recent::load(&app).items
}

/// 从最近列表移除一项（不动磁盘上的站点）。
#[tauri::command]
pub fn forget_project(app: AppHandle, path: PathBuf) -> Result<Vec<recent::RecentEntry>> {
    let mut list = recent::load(&app);
    list.remove(&path);
    recent::save(&app, &list)?;
    Ok(list.items)
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    state.close();
}

#[tauri::command]
pub fn project_summary(state: State<'_, AppState>) -> Result<ProjectSummary> {
    state.with_session(|session| {
        let builder = &session.builder;
        Ok(ProjectSummary {
            root: session.root.clone(),
            config: builder.config.clone(),
            pages: page_summaries(builder),
            layouts: builder.templates().layouts().into_iter().cloned().collect(),
            components: builder
                .templates()
                .components()
                .into_iter()
                .cloned()
                .collect(),
            templates: builder.templates().infos().cloned().collect(),
            recent_builds: builder.index().recent_builds(10)?,
        })
    })
}

// ---------------------------------------------------------------- 配置

#[tauri::command]
pub fn read_config(state: State<'_, AppState>) -> Result<SiteConfig> {
    state.with_session(|session| Ok(session.builder.config.clone()))
}

/// 保存配置并重新加载项目（`page_size`、目录变更都会影响构建）。
#[tauri::command]
pub fn save_config(state: State<'_, AppState>, config: SiteConfig) -> Result<Vec<String>> {
    let issues = config.validate();
    if !issues.is_empty() {
        return Err(AppError::Message(issues.join("；")));
    }
    state.with_session_mut(|session| {
        config.save(&session.root)?;
        session.builder = staticsmith_core::Builder::open(&session.root)?;
        Ok(Vec::new())
    })
}

/// `staticsmith.toml` 的原文，供「源码」分段编辑。
///
/// 读文件而不是把 `SiteConfig` 序列化回去：源码视图要给出的是**磁盘上那份文本**，
/// 连注释与键序一起。序列化出来的那份长得不一样，用户会以为文件被改过。
#[tauri::command]
pub fn read_config_source(state: State<'_, AppState>) -> Result<String> {
    state.with_session(|session| {
        let path = session.root.join(staticsmith_core::CONFIG_FILE_NAME);
        Ok(std::fs::read_to_string(&path)?)
    })
}

/// 保存 `staticsmith.toml` 原文。
///
/// **逐字写盘**——源码视图的意义就在于「我写的就是文件里的」，不做格式化、不重排键。
/// 但写之前必须先解析 + 校验：写进一份解析不了的配置，下一次打开站点就直接报错，
/// 而那时界面已经帮不上忙（配置读不出来，连设置页都进不去）。
#[tauri::command]
pub fn save_config_source(state: State<'_, AppState>, raw: String) -> Result<Vec<String>> {
    let config = SiteConfig::parse(&raw)?;
    let issues = config.validate();
    if !issues.is_empty() {
        return Err(AppError::Message(issues.join("；")));
    }
    state.with_session_mut(|session| {
        let path = session.root.join(staticsmith_core::CONFIG_FILE_NAME);
        std::fs::write(&path, &raw)?;
        session.builder = staticsmith_core::Builder::open(&session.root)?;
        Ok(Vec::new())
    })
}

// ---------------------------------------------------------------- 内容

#[tauri::command]
pub fn list_pages(state: State<'_, AppState>) -> Result<Vec<PageSummary>> {
    state.with_session(|session| Ok(page_summaries(&session.builder)))
}

/// 全文搜索：在标题与正文里找一个词，返回带上下文的片段。
///
/// 与 MCP 的 `search_content` 同源（`staticsmith_core::search`）：
/// 界面里搜到的和 AI Agent 搜到的必须是同一批，否则「你说有我搜不到」。
/// 侧栏原先只按标题与路径过滤已加载的清单，找不到「上次写过某个词的那篇」。
#[tauri::command]
pub fn search_content(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>> {
    state.with_session(|session| {
        Ok(staticsmith_core::search::search(
            session.builder.pages(),
            &query,
            limit.unwrap_or(30),
        ))
    })
}

/// 读取内容源文件原文（含 front matter），供编辑器打开。
#[tauri::command]
pub fn read_content(state: State<'_, AppState>, source: String) -> Result<String> {
    state.with_session(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &source)?;
        Ok(std::fs::read_to_string(path)?)
    })
}

/// 保存内容并返回增量构建计划，界面据此提示「影响 N 个页面」。
#[tauri::command]
pub fn save_content(state: State<'_, AppState>, args: SaveContentArgs) -> Result<BuildPlan> {
    state.with_session_mut(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &args.source)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 先登记再写：监听事件最快也要等去抖窗口，不会早于这里。
        state.note_self_write(&path);
        std::fs::write(&path, &args.raw)?;
        session.builder.reload()?;
        Ok(session.builder.plan(BuildMode::Incremental)?)
    })
}

#[tauri::command]
pub fn delete_content(state: State<'_, AppState>, source: String) -> Result<BuildPlan> {
    state.with_session_mut(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &source)?;
        state.note_self_write(&path);
        std::fs::remove_file(&path)?;
        session.builder.reload()?;
        Ok(session.builder.plan(BuildMode::Incremental)?)
    })
}

/// 在父级布局下渲染单页，用于「所见即所得」预览。
///
/// 本地样式与图片已经内联进 HTML（`srcdoc` 沙箱取不到磁盘文件），
/// 顺带回传「换了几处、放弃了几处」，界面据此说明这一屏是不是完整效果。
#[tauri::command]
pub fn preview_page(state: State<'_, AppState>, source: String) -> Result<PreviewPage> {
    state.with_session(|session| {
        let (html, inlined) = session.builder.preview(&source)?;
        Ok(PreviewPage { html, inlined })
    })
}

/// 一次内存预览的结果。
#[derive(Debug, Serialize)]
pub struct PreviewPage {
    pub html: String,
    pub inlined: PreviewInlined,
}

/// 读出源文里的 front matter 字段，供属性面板回填。
///
/// 不落盘、不看会话：编辑器缓冲区里的文本才是当前真相，可能还没保存。
#[tauri::command]
pub fn read_front_matter(raw: String) -> Result<FrontMatter> {
    Ok(frontmatter::read(&raw)?)
}

/// 把属性面板的改动折算成新的源文，正文与未涉及的键原样保留。
#[tauri::command]
pub fn apply_front_matter(raw: String, patch: frontmatter::Patch) -> Result<String> {
    Ok(frontmatter::apply(&raw, &patch)?)
}
/// 新建内容，返回其相对 `content/` 的路径。
#[tauri::command]
pub fn create_content(state: State<'_, AppState>, request: NewContent) -> Result<String> {
    state.with_session_mut(|session| {
        let source = session.builder.create_content(&request)?;
        // 写盘已发生，但监听器还在去抖窗口里，此时登记仍能对消。
        state.note_self_write(&content::resolve_source(
            &session.builder.paths.content,
            &source,
        )?);
        Ok(source)
    })
}

// ---------------------------------------------------------------- 本地预览服务器

/// 启动本地预览服务器，返回站点根地址。
///
/// 内存预览（`preview_page`）取不到图片与 CSS——iframe 的 `srcdoc` 没有本地文件访问权限。
/// 这个服务器只监听 127.0.0.1，指向产物目录，因此预览与线上完全一致。
#[tauri::command]
pub fn start_preview_server(state: State<'_, AppState>, port: Option<u16>) -> Result<String> {
    state.with_session_mut(|session| {
        if let Some(server) = &session.preview {
            return Ok(server.base_url());
        }
        let server = PreviewServer::start(&session.builder.paths.output, port.unwrap_or(0))?;
        let url = server.base_url();
        session.preview = Some(server);
        Ok(url)
    })
}

#[tauri::command]
pub fn stop_preview_server(state: State<'_, AppState>) -> Result<()> {
    state.with_session_mut(|session| {
        // drop 即停止：PreviewServer 的 Drop 会置停止标记并 join 线程。
        session.preview = None;
        Ok(())
    })
}

/// 当前预览地址，未启动时返回 null。
#[tauri::command]
pub fn preview_server_url(state: State<'_, AppState>) -> Result<Option<String>> {
    state.with_session(|session| Ok(session.preview.as_ref().map(|s| s.base_url())))
}

// ---------------------------------------------------------------- AI 接入（MCP）

/// MCP 服务端的运行状态，给界面显示与复制端点用。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub port: u16,
    /// 客户端配置里填的那个地址（POST）。
    pub endpoint: String,
    /// 需要服务端推送时用这个（SSE）。
    pub sse_endpoint: String,
    pub allow_write: bool,
    pub allow_deploy: bool,
}

fn mcp_status(server: &staticsmith_mcp::McpHttpServer, perms: McpPermissions) -> McpStatus {
    McpStatus {
        port: server.port(),
        endpoint: server.mcp_endpoint(),
        sse_endpoint: server.sse_endpoint(),
        allow_write: perms.write,
        allow_deploy: perms.deploy,
    }
}

/// 启动 MCP 服务端的参数。与其他多参数命令一样走 `args` 结构体，
/// 免得依赖 Tauri 对 camelCase 参数名的自动转换。
#[derive(Debug, Deserialize)]
pub struct McpStartArgs {
    pub allow_write: bool,
    pub allow_deploy: bool,
    pub port: Option<u16>,
}

/// 启动 MCP 服务端，让 AI Agent 能操作这个站点。
///
/// 以前这件事只有 CLI 能做（`staticsmith mcp`），而「把重复劳动交给 AI」是这个产品
/// 主推的用法之一——预设用户要开终端才用得上，等于这条路不存在。
///
/// 权限默认只读，写入与发布要显式打开：Agent 会误删、会把没写完的稿子发上线，
/// 这两件事都不可逆。只绑 127.0.0.1（`serve_http` 保证），不暴露到局域网。
#[tauri::command]
pub fn start_mcp_server(state: State<'_, AppState>, args: McpStartArgs) -> Result<McpStatus> {
    state.with_session_mut(|session| {
        // 已经开着就先停掉再按新权限起：权限是启动参数，改权限只能重启。
        // 直接返回旧状态会出现「界面上勾了写入，实际还是只读」。
        session.mcp = None;
        let perms = McpPermissions {
            write: args.allow_write,
            deploy: args.allow_deploy,
        };
        let server = staticsmith_mcp::McpServer::open(&session.root, perms)?;
        let http =
            staticsmith_mcp::serve_http(std::sync::Arc::new(server), args.port.unwrap_or(0))?;
        let status = mcp_status(&http, perms);
        session.mcp = Some(http);
        Ok(status)
    })
}

#[tauri::command]
pub fn stop_mcp_server(state: State<'_, AppState>) -> Result<()> {
    state.with_session_mut(|session| {
        // drop 即停止：McpHttpServer 的 Drop 会置停止标记并 join 线程
        session.mcp = None;
        Ok(())
    })
}

/// 当前 MCP 状态，未启动时返回 null。
///
/// 权限从服务端自己那份配置读回来，不由前端记着——前端记的话，
/// 重启应用后界面会显示上一次的勾选，而服务端其实没在跑。
#[tauri::command]
pub fn mcp_server_status(state: State<'_, AppState>) -> Result<Option<McpStatus>> {
    state.with_session(|session| {
        Ok(session
            .mcp
            .as_ref()
            .map(|http| mcp_status(http, http.permissions())))
    })
}

// ---------------------------------------------------------------- 导入

/// 扫描待导入目录：每篇会写到哪、front matter 变成什么样、哪里需要人看一下。只读。
#[tauri::command]
pub fn scan_import(
    state: State<'_, AppState>,
    dir: String,
    section: String,
) -> Result<Vec<ImportCandidate>> {
    state.with_session(|session| Ok(session.builder.scan_import(Path::new(&dir), &section)?))
}

/// 导入内容。目标已存在的跳过，不覆盖；正文原样保留。
#[tauri::command]
pub fn import_content(
    state: State<'_, AppState>,
    dir: String,
    section: String,
) -> Result<ImportReport> {
    state.with_session_mut(|session| {
        let from = Path::new(&dir);
        // 先扫一遍拿到会写哪些文件，逐个登记自身写入：一次导入可能上百个文件，
        // 不登记的话监听器会把它们当成外部改动，界面立刻弹横幅
        let content_root = session.builder.paths.content.clone();
        for candidate in session.builder.scan_import(from, &section)? {
            if candidate.importable {
                state.note_self_write(&content::resolve_source(&content_root, &candidate.target)?);
            }
        }
        Ok(session.builder.import_content(from, &section)?)
    })
}

// ---------------------------------------------------------------- 批量动作

/// 批量增删标签的参数。
#[derive(Debug, Deserialize)]
pub struct BatchTagsArgs {
    pub sources: Vec<String>,
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// 批量搬动的参数。
#[derive(Debug, Deserialize)]
pub struct BatchMoveArgs {
    pub sources: Vec<String>,
    pub to_section: String,
    /// 省略时按 true：搬动会改 URL，不补旧地址等于打断外部链接。
    #[serde(default = "keep_aliases_default")]
    pub keep_aliases: bool,
}

/// 批量改 front matter 的结果 + 增量计划。
///
/// 顺带回传计划，界面就能立刻更新「待生成」标记，不必再单独发一次请求。
#[derive(Debug, Serialize)]
pub struct BatchReport {
    pub changed: Vec<String>,
    pub skipped: Vec<BatchSkipped>,
    pub plan: BuildPlan,
}

/// 批量搬动的结果 + 增量计划。
#[derive(Debug, Serialize)]
pub struct BatchMoveReport {
    pub moved: Vec<BatchMoved>,
    pub skipped: Vec<BatchSkipped>,
    pub plan: BuildPlan,
}

/// 批量增删标签。加什么、去什么分开传：整集合覆盖会把各篇原有的标签洗掉。
#[tauri::command]
pub fn batch_edit_tags(state: State<'_, AppState>, args: BatchTagsArgs) -> Result<BatchReport> {
    state.with_session_mut(|session| {
        note_batch(&state, session, &args.sources);
        let out = session.builder.batch_edit_tags(
            &args.sources,
            &TagEdit {
                add: args.add,
                remove: args.remove,
            },
        )?;
        Ok(BatchReport {
            changed: out.changed,
            skipped: out.skipped,
            plan: session.builder.plan(BuildMode::Incremental)?,
        })
    })
}

/// 批量发布 / 收回草稿。
#[tauri::command]
pub fn batch_set_draft(
    state: State<'_, AppState>,
    sources: Vec<String>,
    draft: bool,
) -> Result<BatchReport> {
    state.with_session_mut(|session| {
        note_batch(&state, session, &sources);
        let out = session.builder.batch_set_draft(&sources, draft)?;
        Ok(BatchReport {
            changed: out.changed,
            skipped: out.skipped,
            plan: session.builder.plan(BuildMode::Incremental)?,
        })
    })
}

/// 批量搬到另一个栏目，默认补旧地址。
#[tauri::command]
pub fn batch_move(state: State<'_, AppState>, args: BatchMoveArgs) -> Result<BatchMoveReport> {
    state.with_session_mut(|session| {
        note_batch(&state, session, &args.sources);
        // 目标目录会新增文件，一并登记，免得改完弹「检测到外部修改」
        let target = content::resolve_source(&session.builder.paths.content, &args.to_section)?;
        state.note_self_tree(&target);
        let out = session
            .builder
            .batch_move(&args.sources, &args.to_section, args.keep_aliases)?;
        Ok(BatchMoveReport {
            moved: out.moved,
            skipped: out.skipped,
            plan: session.builder.plan(BuildMode::Incremental)?,
        })
    })
}

/// 批量删除内容。不可逆，界面必须先二次确认。
#[tauri::command]
pub fn batch_delete(state: State<'_, AppState>, sources: Vec<String>) -> Result<BatchReport> {
    state.with_session_mut(|session| {
        note_batch(&state, session, &sources);
        let out = session.builder.batch_delete(&sources)?;
        Ok(BatchReport {
            changed: out.changed,
            skipped: out.skipped,
            plan: session.builder.plan(BuildMode::Incremental)?,
        })
    })
}

/// 干跑一个批量动作：算出每篇会发生什么，不碰磁盘。
///
/// 只有搬动与删除会走这一步——它们不可逆或会改地址；加标签、切草稿反手就能改回来，
/// 多一次确认只是白点一下。
#[tauri::command]
pub fn batch_preview(
    state: State<'_, AppState>,
    sources: Vec<String>,
    action: BatchActionArgs,
) -> Result<BatchPreview> {
    state.with_session(|session| Ok(session.builder.batch_preview(&sources, &action.into())?))
}

/// 界面传过来的动作。用带标签的枚举，前端只发一个 `kind` 字段就能选中分支。
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchActionArgs {
    Tags {
        #[serde(default)]
        add: Vec<String>,
        #[serde(default)]
        remove: Vec<String>,
    },
    Draft {
        draft: bool,
    },
    Move {
        to_section: String,
    },
    Delete,
}

impl From<BatchActionArgs> for BatchAction {
    fn from(args: BatchActionArgs) -> Self {
        match args {
            BatchActionArgs::Tags { add, remove } => BatchAction::Tags(TagEdit { add, remove }),
            BatchActionArgs::Draft { draft } => BatchAction::Draft(draft),
            BatchActionArgs::Move { to_section } => BatchAction::Move { to_section },
            BatchActionArgs::Delete => BatchAction::Delete,
        }
    }
}

/// 批量动作会写很多文件，逐个登记自身写入，避免监听器把它们当成外部改动。
///
/// 越界的 source 在这里静默跳过：登记只是给文件监听器消噪，真正的拦截在
/// `content::resolve_source` 里，动作本身会带着原因失败。
fn note_batch(state: &State<'_, AppState>, session: &Session, sources: &[String]) {
    for source in sources {
        if let Ok(path) = content::resolve_source(&session.builder.paths.content, source) {
            state.note_self_write(&path);
        }
    }
}

// ---------------------------------------------------------------- 跨文件替换

/// 界面传过来的替换请求。
///
/// `sources` 为空表示全站——界面上那是一个显式的二选一（「全站」/「选中的 N 篇」），
/// 空清单只在选了「全站」时才发得出来。
#[derive(Debug, Deserialize)]
pub struct ReplaceArgs {
    pub find: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl ReplaceArgs {
    fn split(self) -> (ReplaceScope, ReplaceRule) {
        let scope = if self.sources.is_empty() {
            ReplaceScope::All
        } else {
            ReplaceScope::Only(self.sources)
        };
        (
            scope,
            ReplaceRule {
                find: self.find,
                replace: self.replace,
                ignore_case: self.ignore_case,
            },
        )
    }
}

/// 替换结果 + 增量计划，界面拿到就能更新「待生成」标记。
#[derive(Debug, Serialize)]
pub struct ReplaceReport {
    #[serde(flatten)]
    pub result: ReplaceResult,
    pub plan: BuildPlan,
}

/// 干跑一次跨文件替换：哪几篇、共几处、每处前后长什么样。不碰磁盘。
#[tauri::command]
pub fn preview_replace(state: State<'_, AppState>, args: ReplaceArgs) -> Result<ReplaceResult> {
    let (scope, rule) = args.split();
    state.with_session(|session| Ok(session.builder.preview_replace(&scope, &rule)?))
}

/// 执行跨文件替换。界面必须先让人看过干跑结果——正文替换没有撤销栈。
#[tauri::command]
pub fn apply_replace(state: State<'_, AppState>, args: ReplaceArgs) -> Result<ReplaceReport> {
    let (scope, rule) = args.split();
    state.with_session_mut(|session| {
        // 改哪几篇要等跑完才知道，所以整棵内容树先登记为自身写入，
        // 否则监听器会把这一批改动当成外部修改，弹一堆「磁盘上变了」
        let root = session.builder.paths.content.clone();
        state.note_self_tree(&root);
        let result = session.builder.apply_replace(&scope, &rule)?;
        Ok(ReplaceReport {
            result,
            plan: session.builder.plan(BuildMode::Incremental)?,
        })
    })
}

// ---------------------------------------------------------------- 栏目

/// 栏目清单（含根目录）。
#[tauri::command]
pub fn list_sections(state: State<'_, AppState>) -> Result<Vec<Section>> {
    state.with_session(|session| Ok(session.builder.sections()))
}

/// 新建栏目：建目录并写一张索引页。
#[tauri::command]
pub fn create_section(
    state: State<'_, AppState>,
    path: String,
    title: String,
) -> Result<SectionCreated> {
    state.with_session_mut(|session| {
        let created = session.builder.create_section(&path, &title)?;
        let index = content::resolve_source(&session.builder.paths.content, &created.index_source)?;
        state.note_self_write(&index);
        Ok(created)
    })
}

/// 栏目改名。`keep_aliases` 为真时给每篇文章补旧地址，老链接经重定向页继续可用。
#[tauri::command]
pub fn rename_section(
    state: State<'_, AppState>,
    args: RenameSectionArgs,
) -> Result<SectionRenamed> {
    state.with_session_mut(|session| {
        let content_root = session.builder.paths.content.clone();
        // 整棵子树的变更都是自己造成的，别让「检测到外部修改」在改名后弹出来
        for section in [&args.from, &args.to] {
            let dir = content::resolve_source(&content_root, section)?;
            state.note_self_tree(&dir);
        }
        Ok(session
            .builder
            .rename_section(&args.from, &args.to, args.keep_aliases)?)
    })
}

/// 删除空栏目。里面还有文章时报错，不会连带删除。返回删除后的栏目清单。
#[tauri::command]
pub fn remove_section(state: State<'_, AppState>, path: String) -> Result<Vec<Section>> {
    state.with_session_mut(|session| {
        let dir = content::resolve_source(&session.builder.paths.content, &path)?;
        state.note_self_tree(&dir);
        session.builder.remove_section(&path)?;
        Ok(session.builder.sections())
    })
}

/// 改栏目元信息（标题、简介、排序权重），写在索引页的 front matter 上。
///
/// 缺索引页的栏目会顺手补一张——不然元信息没处存。返回刷新后的栏目清单。
#[tauri::command]
pub fn save_section_meta(
    state: State<'_, AppState>,
    args: SectionMetaArgs,
) -> Result<Vec<Section>> {
    state.with_session_mut(|session| {
        let content_root = session.builder.paths.content.clone();
        // 索引页可能还不存在，两个候选名都先记下，免得补出来的文件被当成外部修改
        for name in ["index.md", "_index.md"] {
            let file = content::resolve_source(
                &content_root,
                &if args.path.is_empty() {
                    name.to_string()
                } else {
                    format!("{}/{name}", args.path)
                },
            )?;
            state.note_self_write(&file);
        }
        session.builder.set_section_meta(&args.path, &args.meta)?;
        Ok(session.builder.sections())
    })
}

// ---------------------------------------------------------------- 主题包

/// 打包当前站点的外观（模板 + 主题静态资源）成一个 zip。
///
/// 不含 `content/` 与 `static/`：文章与上传的图片是站点的，不是主题的。
#[tauri::command]
pub fn export_theme(state: State<'_, AppState>, args: ThemeExportArgs) -> Result<ThemeExported> {
    state.with_session(|session| {
        Ok(session
            .builder
            .export_theme(Path::new(&args.archive), &args.manifest)?)
    })
}

/// 读出主题包会写哪些文件、哪些会覆盖现有文件。只读，不碰磁盘上的模板。
#[tauri::command]
pub fn scan_theme(state: State<'_, AppState>, archive: String) -> Result<ThemePreview> {
    state.with_session(|session| Ok(session.builder.scan_theme(Path::new(&archive))?))
}

/// 装主题包。`overwrite` 为假时已存在的文件一律跳过。
#[tauri::command]
pub fn import_theme(
    state: State<'_, AppState>,
    archive: String,
    overwrite: bool,
) -> Result<ThemeImported> {
    state.with_session_mut(|session| {
        // 一次装十几个模板，逐个记不如把两棵子树都标上：否则装完必弹「检测到外部修改」
        state.note_self_tree(&session.builder.paths.templates);
        state.note_self_tree(&session.builder.paths.theme);
        Ok(session
            .builder
            .import_theme(Path::new(&archive), overwrite)?)
    })
}

// ---------------------------------------------------------------- 媒体资源

/// 保存编辑器粘贴或拖入的文件，返回可直接写进 Markdown 的地址。
///
/// 二进制经 base64 传输：Tauri 的 JSON IPC 传字节数组会膨胀数倍，
/// base64 只有 4/3 的开销，且两端行为确定。
#[tauri::command]
pub fn save_asset(
    state: State<'_, AppState>,
    file_name: String,
    data_base64: String,
) -> Result<SavedAsset> {
    let bytes = BASE64
        .decode(data_base64.as_bytes())
        .map_err(|e| AppError::Message(format!("资源数据不是合法的 base64: {e}")))?;
    state.with_session(|session| Ok(session.builder.save_asset(&bytes, &file_name)?))
}

/// 已登记的媒体资源列表，供媒体库浏览。
#[tauri::command]
pub fn list_assets(state: State<'_, AppState>) -> Result<Vec<AssetRecord>> {
    state.with_session(|session| Ok(session.builder.assets()?))
}

/// SEO 体检：标题、描述、关键词、重复内容与站点级配置。
///
/// 与 MCP 的 `audit_seo` 同源，界面与 AI Agent 看到的是同一份结论。
#[tauri::command]
pub fn audit_seo(state: State<'_, AppState>) -> Result<SeoReport> {
    state.with_session(|session| Ok(session.builder.audit_seo()))
}

/// 媒体资源体检：没人引用的文件与引用了却不存在的地址。
#[tauri::command]
pub fn audit_media(state: State<'_, AppState>) -> Result<MediaReport> {
    state.with_session(|session| Ok(session.builder.audit_media()?))
}

/// 站内链接体检：点了会 404 的链接。读产物，因此需要先生成一次。
#[tauri::command]
pub fn audit_links(state: State<'_, AppState>) -> Result<LinkReport> {
    state.with_session(|session| Ok(session.builder.audit_links()?))
}

/// 删除媒体文件。不可撤销，界面需先二次确认。
#[tauri::command]
pub fn remove_media(state: State<'_, AppState>, paths: Vec<String>) -> Result<MediaRemoved> {
    state.with_session(|session| Ok(session.builder.remove_media(&paths)?))
}

/// 产物清单（页面 / 标签页 / 分页 / sitemap / 订阅 / 静态资源）。
///
/// 内容列表只反映 `content/` 下的 Markdown，而标签页、分页页这些「非内容页」
/// 之前在界面里没有任何入口。还没生成过时返回空列表。
#[tauri::command]
pub fn list_outputs(state: State<'_, AppState>) -> Result<Vec<OutputFile>> {
    state.with_session(|session| Ok(session.builder.outputs()?))
}

// ---------------------------------------------------------------- 模板与组件

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> Result<Vec<TemplateInfo>> {
    state.with_session(|session| Ok(session.builder.templates().infos().cloned().collect()))
}

/// 布局 → 组件的树形结构，供「布局管理器」展示继承关系。
#[tauri::command]
pub fn template_tree(state: State<'_, AppState>, root: String) -> Result<TemplateNode> {
    state.with_session(|session| Ok(session.builder.templates().graph.tree(&root)))
}

#[tauri::command]
pub fn read_template(state: State<'_, AppState>, name: String) -> Result<String> {
    state.with_session(|session| {
        let info = session
            .builder
            .templates()
            .get(&name)
            .ok_or_else(|| AppError::Message(format!("模板不存在: {name}")))?;
        Ok(std::fs::read_to_string(&info.path)?)
    })
}

/// 保存模板（例如可视化修改了头部导航），返回级联影响范围。
#[tauri::command]
pub fn save_template(
    state: State<'_, AppState>,
    name: String,
    source: String,
) -> Result<BuildPlan> {
    state.with_session_mut(|session| {
        let path = templates::resolve_template(&session.builder.paths.templates, &name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        state.note_self_write(&path);
        std::fs::write(&path, source)?;
        session.builder.reload()?;
        Ok(session.builder.plan(BuildMode::Incremental)?)
    })
}

// ---------------------------------------------------------------- 构建

#[tauri::command]
pub fn build_plan(state: State<'_, AppState>, mode: BuildMode) -> Result<BuildPlan> {
    state.with_session(|session| Ok(session.builder.plan(mode)?))
}

/// 执行构建。开始与结束时各发一次 `build://progress` 事件驱动进度条。
#[tauri::command]
pub fn run_build(
    window: Window,
    state: State<'_, AppState>,
    mode: BuildMode,
) -> Result<BuildReport> {
    state.with_session_mut(|session| {
        session.builder.reload()?;
        let plan = session.builder.plan(mode)?;
        let total = plan.pages.len();
        emit(
            &window,
            EVENT_BUILD_PROGRESS,
            BuildProgress {
                phase: "渲染中".to_string(),
                current: 0,
                total,
            },
        );

        let report = session.builder.build(mode)?;
        // 产物变了就提一下版本号：预览页面里的脚本据此自己刷新，并保留滚动位置
        if let Some(server) = &session.preview {
            server.bump();
        }
        emit(
            &window,
            EVENT_BUILD_PROGRESS,
            BuildProgress {
                phase: "完成".to_string(),
                current: report.pages_rendered,
                total,
            },
        );
        Ok(report)
    })
}

/// 输出目录绝对路径，供前端调用 opener 插件在文件管理器中打开。
#[tauri::command]
pub fn output_dir(state: State<'_, AppState>) -> Result<PathBuf> {
    state.with_session(|session| Ok(session.builder.paths.output.clone()))
}

// ---------------------------------------------------------------- 发布

/// 发布站点。凭证从系统凭据管理器读取，不经过配置文件。
#[tauri::command]
pub fn deploy_site(window: Window, state: State<'_, AppState>) -> Result<DeployReport> {
    let (config, dist) = state.with_session(|session| {
        Ok((
            session.builder.config.clone(),
            session.builder.paths.output.clone(),
        ))
    })?;

    let credentials = resolve_credentials(&config)?;
    let deployer = staticsmith_deploy::from_config(&config, credentials)?;
    deployer.check()?;

    let target = window.clone();
    let mut on_progress = move |p: Progress| emit(&target, EVENT_DEPLOY_PROGRESS, p);
    Ok(deployer.deploy(&dist, &mut on_progress)?)
}

/// 仅做连接与凭证检查。
#[tauri::command]
pub fn check_deploy(state: State<'_, AppState>) -> Result<()> {
    let config = state.with_session(|session| Ok(session.builder.config.clone()))?;
    let credentials = resolve_credentials(&config)?;
    staticsmith_deploy::from_config(&config, credentials)?.check()?;
    Ok(())
}

/// 把密码 / Token 写入操作系统凭据管理器。
#[tauri::command]
pub fn save_secret(account: String, secret: String) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, &account)?.set_password(&secret)?;
    Ok(())
}

#[tauri::command]
pub fn has_secret(account: String) -> bool {
    keyring::Entry::new(KEYRING_SERVICE, &account)
        .and_then(|e| e.get_password())
        .is_ok()
}

#[tauri::command]
pub fn delete_secret(account: String) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, &account)?.delete_credential()?;
    Ok(())
}

/// 凭证解析优先级：系统凭据管理器 → 环境变量（FTP 的 `password_env`）→ 无认证。
fn resolve_credentials(config: &SiteConfig) -> Result<Credentials> {
    use staticsmith_core::config::DeployKind;

    // 条目名走 account_for_config：解析凭证与界面查询必须问同一个名字
    let account = account_for_config(config);
    match config.deploy.r#type {
        DeployKind::None => Ok(Credentials::None),
        DeployKind::Git => {
            let git = config
                .deploy
                .git
                .as_ref()
                .ok_or_else(|| AppError::Message("缺少 [deploy.git] 配置".into()))?;
            if git.auth_type == "ssh" {
                let key = git.ssh_key_path.clone().ok_or_else(|| {
                    AppError::Message("auth_type = \"ssh\" 时需要填写 ssh_key_path".into())
                })?;
                return Ok(Credentials::SshKey {
                    username: "git".to_string(),
                    private_key: key,
                    passphrase: read_secret(&account),
                });
            }
            let token = read_secret(&account).ok_or_else(|| {
                AppError::Message("未找到 Git Token，请先在「发布设置」中保存凭证".to_string())
            })?;
            Ok(Credentials::UserPassword {
                // GitHub / GitLab 的 Token 认证接受任意用户名。
                username: "staticsmith".to_string(),
                password: token,
            })
        }
        DeployKind::Ftp => {
            let ftp = config
                .deploy
                .ftp
                .as_ref()
                .ok_or_else(|| AppError::Message("缺少 [deploy.ftp] 配置".into()))?;
            let password = read_secret(&account)
                .or_else(|| {
                    ftp.password_env
                        .as_ref()
                        .and_then(|k| std::env::var(k).ok())
                })
                .ok_or_else(|| {
                    AppError::Message(format!(
                        "未找到 {} 的密码：请保存到凭据管理器，或设置环境变量 {}",
                        ftp.host,
                        ftp.password_env.as_deref().unwrap_or("FTP_PASSWORD")
                    ))
                })?;
            Ok(Credentials::UserPassword {
                username: ftp.username.clone(),
                password,
            })
        }
    }
}

/// 凭据条目名的唯一定义处。
///
/// 界面此前自己拼 `git:${remote}`，与这里各写一份。这种重复最难发现：
/// 改了规则之后保存写进 A、查询读的是 B，而「凭证在不在」这件事恰恰只能靠这个名字判断，
/// 用户看到的是「明明保存过，界面说没有」。现在界面走 `deploy_account` 问。
fn account_for_config(config: &SiteConfig) -> String {
    use staticsmith_core::config::DeployKind;

    match config.deploy.r#type {
        DeployKind::None => String::new(),
        DeployKind::Git => config
            .deploy
            .git
            .as_ref()
            .map(|git| account_for_git(&git.remote))
            .unwrap_or_default(),
        DeployKind::Ftp => config
            .deploy
            .ftp
            .as_ref()
            .map(|ftp| account_for_ftp(&ftp.host, &ftp.username))
            .unwrap_or_default(),
    }
}

/// 当前站点的凭据条目名。未配置发布方式时是空串。
#[tauri::command]
pub fn deploy_account(state: State<'_, AppState>) -> Result<String> {
    state.with_session(|session| Ok(account_for_config(&session.builder.config)))
}

fn read_secret(account: &str) -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .ok()?
        .get_password()
        .ok()
}

/// 凭据条目命名规则，前端保存凭证时需要使用同样的规则。
pub fn account_for_git(remote: &str) -> String {
    format!("git:{remote}")
}

pub fn account_for_ftp(host: &str, username: &str) -> String {
    format!("ftp:{username}@{host}")
}

// ---------------------------------------------------------------- 内部工具

fn page_summary(page: &staticsmith_core::Page) -> PageSummary {
    PageSummary {
        source: page.source.clone(),
        title: page.title.clone(),
        url: page.url.clone(),
        template: page.template.clone(),
        section: page.section.clone(),
        is_index: page.is_index,
        draft: page.draft,
        date: page.date.map(|d| d.to_rfc3339()),
        scheduled: false,
        tags: page.tags.clone(),
    }
}

/// 页面摘要列表。
///
/// `scheduled` 要知道「这次哪些页面不会进产物」，判断交给 `Builder::published_pages`——
/// 界面不再自己比一遍日期，免得和构建给出两套结论。
fn page_summaries(builder: &staticsmith_core::Builder) -> Vec<PageSummary> {
    let published: std::collections::BTreeSet<&str> = builder
        .published_pages()
        .iter()
        .map(|p| p.source.as_str())
        .collect();
    builder
        .pages()
        .iter()
        .map(|page| {
            let mut summary = page_summary(page);
            summary.scheduled = !page.draft && !published.contains(page.source.as_str());
            summary
        })
        .collect()
}

/// 事件只投给发起操作的窗口。
///
/// 用 `emit_to` 而不是广播：多窗口下广播会让 A 站点的构建进度出现在 B 站点界面上。
/// 发送失败不应中断业务流程，只记日志。
fn emit<T: Serialize + Clone>(window: &Window, event: &str, payload: T) {
    if let Err(err) = window.emit_to(window.label(), event, payload) {
        tracing::warn!("发送事件 {event} 失败: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_account_names_are_stable() {
        assert_eq!(
            account_for_git("https://github.com/u/r.git"),
            "git:https://github.com/u/r.git"
        );
        assert_eq!(
            account_for_ftp("ftp.example.com", "alice"),
            "ftp:alice@ftp.example.com"
        );
    }

    /// 界面查的条目名与解析凭证用的必须是同一个。
    ///
    /// 这条以前靠「前端记得照抄规则」保证，而前端确实抄了一份——
    /// 现在两边都走 `account_for_config`，这个测试是那份约定的落点。
    #[test]
    fn account_for_config_covers_every_deploy_kind() {
        use staticsmith_core::config::{DeployKind, FtpDeploy, GitDeploy};

        let mut config = SiteConfig::default();
        assert_eq!(config.deploy.r#type, DeployKind::None);
        assert_eq!(account_for_config(&config), "", "未配置发布方式时没有条目");

        config.deploy.r#type = DeployKind::Git;
        config.deploy.git = Some(GitDeploy {
            remote: "https://github.com/u/r.git".into(),
            branch: "gh-pages".into(),
            commit_message: "publish".into(),
            auth_type: "token".into(),
            ssh_key_path: None,
        });
        assert_eq!(
            account_for_config(&config),
            account_for_git("https://github.com/u/r.git")
        );

        config.deploy.r#type = DeployKind::Ftp;
        config.deploy.ftp = Some(FtpDeploy {
            host: "ftp.example.com".into(),
            port: 21,
            username: "alice".into(),
            password_env: None,
            remote_path: "/public_html".into(),
            sftp: false,
        });
        assert_eq!(
            account_for_config(&config),
            account_for_ftp("ftp.example.com", "alice")
        );
    }

    #[test]
    fn resolve_credentials_returns_none_for_unset_deploy() {
        let config = SiteConfig::default();
        assert!(matches!(
            resolve_credentials(&config).unwrap(),
            Credentials::None
        ));
    }

    #[test]
    fn resolve_credentials_reports_missing_ftp_section() {
        let mut config = SiteConfig::default();
        config.deploy.r#type = staticsmith_core::config::DeployKind::Ftp;
        let err = resolve_credentials(&config).unwrap_err();
        assert!(err.to_string().contains("[deploy.ftp]"));
    }
}
