use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use staticsmith_core::build::{BuildMode, BuildPlan, BuildReport};
use staticsmith_core::graph::TemplateNode;
use staticsmith_core::index::{AssetRecord, BuildRecord};
use staticsmith_core::templates::TemplateInfo;
use staticsmith_core::{content, scaffold, SavedAsset, SiteConfig};
use staticsmith_deploy::{Credentials, DeployReport, Progress};
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, Result};
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
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<ProjectSummary> {
    state.open(&app, &path)?;
    project_summary(state)
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
        std::fs::write(&path, &args.raw)?;
        session.builder.reload()?;
        Ok(session.builder.plan(BuildMode::Incremental)?)
    })
}

#[tauri::command]
pub fn delete_content(state: State<'_, AppState>, source: String) -> Result<BuildPlan> {
    state.with_session_mut(|session| {
        let path = content::resolve_source(&session.builder.paths.content, &source);
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
    app: AppHandle,
    state: State<'_, AppState>,
    mode: BuildMode,
) -> Result<BuildReport> {
    state.with_session_mut(|session| {
        session.builder.reload()?;
        let plan = session.builder.plan(mode)?;
        let total = plan.pages.len();
        emit(
            &app,
            EVENT_BUILD_PROGRESS,
            BuildProgress {
                phase: "渲染中".to_string(),
                current: 0,
                total,
            },
        );

        let report = session.builder.build(mode)?;
        emit(
            &app,
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
pub fn deploy_site(app: AppHandle, state: State<'_, AppState>) -> Result<DeployReport> {
    let (config, dist) = state.with_session(|session| {
        Ok((
            session.builder.config.clone(),
            session.builder.paths.output.clone(),
        ))
    })?;

    let credentials = resolve_credentials(&config)?;
    let deployer = staticsmith_deploy::from_config(&config, credentials)?;
    deployer.check()?;

    let handle = app.clone();
    let mut on_progress = move |p: Progress| emit(&handle, EVENT_DEPLOY_PROGRESS, p);
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
    }
}

/// 事件发送失败不应中断业务流程，只记录日志。
fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(err) = app.emit(event, payload) {
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
