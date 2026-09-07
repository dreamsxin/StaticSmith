use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use staticsmith_core::build::{BuildMode, BuildPlan, BuildReport};
use staticsmith_core::content::FrontMatter;
use staticsmith_core::graph::TemplateNode;
use staticsmith_core::index::{AssetRecord, BuildRecord};
use staticsmith_core::templates::TemplateInfo;
use staticsmith_core::{
    content, frontmatter, scaffold, NewContent, OutputFile, PreviewServer, SavedAsset, SeoReport,
    SiteConfig,
};
use staticsmith_deploy::{Credentials, DeployReport, Progress};
use tauri::{AppHandle, Emitter, Manager, State, Window};

use crate::error::{AppError, Result};
use crate::recent;
use crate::state::{AppState, EVENT_BUILD_PROGRESS, EVENT_DEPLOY_PROGRESS};

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
    /// 索引中被标记为待重新生成的页面。
    pub dirty_pages: Vec<String>,
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
            pages: builder.pages().iter().map(page_summary).collect(),
            layouts: builder.templates().layouts().into_iter().cloned().collect(),
            components: builder
                .templates()
                .components()
                .into_iter()
                .cloned()
                .collect(),
            templates: builder.templates().infos().cloned().collect(),
            recent_builds: builder.index().recent_builds(10)?,
            dirty_pages: builder.index().dirty_pages()?,
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

// ---------------------------------------------------------------- 内容

#[tauri::command]
pub fn list_pages(state: State<'_, AppState>) -> Result<Vec<PageSummary>> {
    state.with_session(|session| Ok(session.builder.pages().iter().map(page_summary).collect()))
}

/// 读取内容源文件原文（含 front matter），供编辑器打开。
#[tauri::command]
pub fn read_content(state: State<'_, AppState>, source: String) -> Result<String> {
    state.with_session(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &source);
        Ok(std::fs::read_to_string(path)?)
    })
}

/// 保存内容并返回增量构建计划，界面据此提示「影响 N 个页面」。
#[tauri::command]
pub fn save_content(state: State<'_, AppState>, args: SaveContentArgs) -> Result<BuildPlan> {
    state.with_session_mut(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &args.source);
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
        let path = content::resolve_source(&session.builder.paths.content, &source);
        state.note_self_write(&path);
        std::fs::remove_file(&path)?;
        session.builder.reload()?;
        Ok(session.builder.plan(BuildMode::Incremental)?)
    })
}

/// 在父级布局下渲染单页，用于「所见即所得」预览。
#[tauri::command]
pub fn preview_page(state: State<'_, AppState>, source: String) -> Result<String> {
    state.with_session(|session| Ok(session.builder.preview(&source)?))
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
        ));
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
        let path = session.builder.paths.templates.join(&name);
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
                    passphrase: read_secret(&account_for_git(&git.remote)),
                });
            }
            let token = read_secret(&account_for_git(&git.remote)).ok_or_else(|| {
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
            let account = account_for_ftp(&ftp.host, &ftp.username);
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
        tags: page.tags.clone(),
    }
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
