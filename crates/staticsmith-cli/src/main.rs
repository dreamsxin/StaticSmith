//! StaticSmith 命令行入口。
//!
//! 定位：桌面端负责可视化编辑，CLI 负责自动化——CI 里构建、服务器上发布、脚本里批量新建内容。
//! 两者共用 `staticsmith-core`，因此行为完全一致，不存在「界面能构建出来、CI 构建不出来」。
//!
//! 凭证策略与桌面端不同：CLI 不碰系统凭据管理器（CI 环境里没有），只读环境变量。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use staticsmith_core::build::{BuildMode, BuildPlan, BuildReport};
use staticsmith_core::{scaffold, Builder, NewContent, PreviewServer};
use staticsmith_deploy::{DeployReport, Progress};
use staticsmith_mcp::{McpServer, Permissions};

#[derive(Debug, Parser)]
#[command(
    name = "staticsmith",
    version,
    about = "本地静站·工坊 命令行工具",
    long_about = "无界面地新建、生成、预览与发布 StaticSmith 站点。适合 CI 与服务器环境。"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args, Clone)]
pub struct ProjectArgs {
    /// 站点根目录，默认当前目录
    #[arg(short, long, default_value = ".", global = true)]
    pub project: PathBuf,
}

/// `audit` 的门禁级别。
///
/// 默认只在「必须修」上失败：把建议项也算成失败，CI 会天天红，
/// 红久了就没人看了。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FailOn {
    /// 有必须修的问题就失败（死链与破图算必须修）
    Error,
    /// 连建议修的问题也算失败
    Warn,
    /// 任何提示都算失败
    Hint,
    /// 只报告，永远返回 0
    Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 在目录中创建站点骨架（已存在的文件不会被覆盖）
    Init {
        /// 目标目录，默认当前目录
        #[arg(default_value = ".")]
        path: PathBuf,
        /// 站点标题
        #[arg(short, long)]
        title: Option<String>,
    },

    /// 新建一篇内容
    New {
        /// 标题
        title: String,
        /// 栏目（相对 content/ 的目录）
        #[arg(short, long, default_value = "posts")]
        section: String,
        /// 自定义文件名主干
        #[arg(long)]
        slug: Option<String>,
        /// 指定页面模板
        #[arg(long)]
        template: Option<String>,
        /// 直接发布，不标记为草稿
        #[arg(long)]
        publish: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 生成站点
    Build {
        /// 全量生成（默认智能增量）
        #[arg(long)]
        full: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 只计算影响范围，不写任何文件
    Plan {
        #[arg(long)]
        full: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 从别的站点导入内容（Hugo / Jekyll 的 YAML front matter → TOML）
    Import {
        /// 待导入的目录，递归找 .md / .markdown
        dir: PathBuf,
        /// 导入到哪个栏目，留空进根目录
        #[arg(long, default_value = "posts")]
        section: String,
        /// 只看会变成什么样，不写文件
        #[arg(long)]
        dry_run: bool,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 启动本地预览服务器（仅监听 127.0.0.1），默认监听文件变更自动重新生成
    Serve {
        /// 端口，0 表示由系统分配
        #[arg(short = 'P', long, default_value_t = 5321)]
        port: u16,
        /// 启动前先生成一次
        #[arg(long)]
        build: bool,
        /// 只做静态服务，不监听文件变更
        #[arg(long)]
        no_watch: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 发布产物（Git 或 FTP/SFTP）
    Deploy {
        /// 只检查连接与凭证，不实际发布
        #[arg(long)]
        check_only: bool,
        /// 发布前先生成一次
        #[arg(long)]
        build: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 校验配置、模板与内容能否正常加载
    Check {
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 体检：SEO 字段、站内死链、媒体资源。可作为 CI 门禁（按 --fail-on 决定退出码）
    Audit {
        /// 只查 SEO 字段
        #[arg(long)]
        seo: bool,
        /// 只查站内死链（读产物，配合 --build）
        #[arg(long)]
        links: bool,
        /// 只查媒体资源（未引用文件与破图）
        #[arg(long)]
        media: bool,
        /// 体检前先生成一次——死链体检读的是产物
        #[arg(long)]
        build: bool,
        /// 输出 JSON，便于脚本处理
        #[arg(long)]
        json: bool,
        /// 达到该级别就以非零码退出
        #[arg(long, value_enum, default_value_t = FailOn::Error)]
        fail_on: FailOn,
        #[command(flatten)]
        project: ProjectArgs,
    },

    /// 启动 MCP 服务端，供 AI Agent 操作站点
    Mcp {
        /// 走 HTTP（POST /mcp 与 GET /sse）而不是 stdio
        #[arg(long)]
        sse: bool,
        /// HTTP 端口，0 表示由系统分配，仅 --sse 时有效
        #[arg(short = 'P', long, default_value_t = 5330)]
        port: u16,
        /// 允许 Agent 写内容与模板、生成产物
        #[arg(long)]
        allow_write: bool,
        /// 允许 Agent 执行发布（会影响线上站点）
        #[arg(long)]
        allow_deploy: bool,
        #[command(flatten)]
        project: ProjectArgs,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "staticsmith=info,staticsmith_core=info".into()),
        )
        .without_time()
        .init();

    run(Cli::parse())
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init { path, title } => cmd_init(&path, title.as_deref()),
        Command::New {
            title,
            section,
            slug,
            template,
            publish,
            project,
        } => {
            let mut request = NewContent::new(title).in_section(section);
            request.slug = slug;
            request.template = template;
            request.draft = !publish;
            cmd_new(&project.project, &request).map(|_| ())
        }
        Command::Build { full, project } => cmd_build(&project.project, mode(full)).map(|_| ()),
        Command::Plan { full, project } => cmd_plan(&project.project, mode(full)).map(|_| ()),
        Command::Import {
            dir,
            section,
            dry_run,
            json,
            project,
        } => cmd_import(&project.project, &dir, &section, dry_run, json),
        Command::Serve {
            port,
            build,
            no_watch,
            project,
        } => cmd_serve(&project.project, port, build, !no_watch),
        Command::Deploy {
            check_only,
            build,
            project,
        } => cmd_deploy(&project.project, check_only, build),
        Command::Check { project } => cmd_check(&project.project),
        Command::Audit {
            seo,
            links,
            media,
            build,
            json,
            fail_on,
            project,
        } => cmd_audit(
            &project.project,
            AuditOptions {
                seo,
                links,
                media,
                build,
                json,
                fail_on,
            },
        ),
        Command::Mcp {
            sse,
            port,
            allow_write,
            allow_deploy,
            project,
        } => cmd_mcp(
            &project.project,
            sse,
            port,
            Permissions {
                write: allow_write,
                deploy: allow_deploy,
            },
        ),
    }
}

fn mode(full: bool) -> BuildMode {
    if full {
        BuildMode::Full
    } else {
        BuildMode::Incremental
    }
}

pub fn cmd_init(path: &PathBuf, title: Option<&str>) -> Result<()> {
    let report = scaffold::init_project(path, title).context("创建站点骨架失败")?;
    println!(
        "已创建 {} 个文件，跳过 {} 个已存在文件：{}",
        report.created.len(),
        report.skipped.len(),
        path.display()
    );
    if !report.created.is_empty() {
        println!("下一步：staticsmith build --project {}", path.display());
    }
    Ok(())
}

pub fn cmd_new(project: &PathBuf, request: &NewContent) -> Result<String> {
    let mut builder = open(project)?;
    let source = builder.create_content(request).context("新建内容失败")?;
    println!("已创建 content/{source}");
    Ok(source)
}

pub fn cmd_build(project: &PathBuf, mode: BuildMode) -> Result<BuildReport> {
    let mut builder = open(project)?;
    let report = builder.build(mode).context("生成失败")?;
    println!(
        "生成完成：{} 个页面、{} 个文件、{} 个静态资源，耗时 {} ms",
        report.pages_rendered, report.files_written, report.assets_copied, report.duration_ms
    );
    if !report.removed_files.is_empty() {
        println!("已清理 {} 个陈旧产物", report.removed_files.len());
    }
    for warning in &report.warnings {
        println!("提示：{warning}");
    }
    Ok(report)
}

pub fn cmd_plan(project: &PathBuf, mode: BuildMode) -> Result<BuildPlan> {
    let builder = open(project)?;
    let plan = builder.plan(mode).context("计算构建计划失败")?;

    println!(
        "待生成 {} / 全站 {} 个页面",
        plan.pages.len(),
        plan.total_pages
    );
    if !plan.changed_templates.is_empty() {
        println!("变更模板：{}", plan.changed_templates.join("、"));
        println!("级联影响：{}", plan.affected_templates.join("、"));
    }
    if !plan.orphaned_pages.is_empty() {
        println!("将清理：{}", plan.orphaned_pages.join("、"));
    }
    for source in &plan.pages {
        println!("  {source}");
    }
    Ok(plan)
}

/// 从别的站点导入内容。
///
/// 迁移是一次性、影响面很大的动作，所以默认也先把「每篇会写到哪、front matter
/// 变成什么样、哪里需要人看一下」打出来；`--dry-run` 则只看不写。
fn cmd_import(
    project: &PathBuf,
    dir: &Path,
    section: &str,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let mut builder = open(project)?;

    if dry_run {
        let candidates = builder.scan_import(dir, section).context("扫描失败")?;
        if json {
            println!("{}", serde_json::to_string_pretty(&candidates)?);
            return Ok(());
        }
        let ready = candidates.iter().filter(|c| c.importable).count();
        println!("找到 {} 篇，可导入 {ready} 篇", candidates.len());
        for candidate in &candidates {
            let mark = if candidate.importable { "+" } else { "!" };
            println!("{mark} {} → {}", candidate.source, candidate.target);
            for warning in &candidate.warnings {
                println!("    {warning}");
            }
            if !candidate.importable {
                println!("    目标已存在，不会覆盖");
            }
        }
        return Ok(());
    }

    let report = builder.import_content(dir, section).context("导入失败")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "导入 {} 篇，跳过 {} 篇",
        report.imported.len(),
        report.skipped.len()
    );
    for source in &report.imported {
        println!("+ {source}");
    }
    for skipped in &report.skipped {
        println!("! {}：{}", skipped.source, skipped.reason);
    }
    if !report.warnings.is_empty() {
        println!("需要人看一下（{} 条）：", report.warnings.len());
        for warning in &report.warnings {
            println!("    {warning}");
        }
    }
    println!("接下来：staticsmith check，再 staticsmith audit 看看 SEO 与死链");
    Ok(())
}

fn cmd_serve(project: &PathBuf, port: u16, build_first: bool, watch: bool) -> Result<()> {
    let mut builder = open(project)?;
    // 监听模式下先生成一次：否则首屏是空目录，得等到第一次改动才有内容。
    if build_first || watch {
        builder.build(BuildMode::Incremental).context("生成失败")?;
    }

    let server = PreviewServer::start(&builder.paths.output, port)
        .context("预览服务器启动失败（端口可能被占用）")?;
    println!("预览地址：{}", server.base_url());

    if !watch {
        println!("按 Ctrl+C 停止（--no-watch：不会自动重新生成）");
        // 服务器在后台线程里跑，主线程挂起等待 Ctrl+C（进程退出时线程随之结束）。
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    println!("正在监听 content/ templates/ themes/，改动后自动增量生成");
    println!("浏览器会自己刷新：预览响应里注入了轮询脚本，产物文件不受影响");
    println!("按 Ctrl+C 停止");
    watch_and_rebuild(&mut builder, &server)
}

/// 监听源文件，改动后增量重建。
///
/// 构建要 `&mut Builder`，而监听回调跑在别的线程上，所以回调只把变更集丢进通道，
/// 真正的构建留在主线程做——顺带保证同一时刻只有一次构建在跑。
fn watch_and_rebuild(builder: &mut Builder, server: &PreviewServer) -> Result<()> {
    use std::sync::mpsc;
    use std::time::Duration;

    use staticsmith_core::watch::{ChangeSet, ProjectWatcher};

    let paths = builder.paths.clone();
    let (tx, rx) = mpsc::channel::<ChangeSet>();
    let _watcher = ProjectWatcher::start(
        &paths.templates,
        &paths.content,
        &paths.theme,
        Duration::from_millis(300),
        move |set| {
            // 主线程已退出时发送失败，忽略即可：进程正在结束。
            let _ = tx.send(set);
        },
    )
    .context("文件监听启动失败")?;

    for set in rx {
        let changed = set.templates.len() + set.content.len() + set.other.len();
        if let Err(err) = builder
            .reload()
            .and_then(|_| builder.build(BuildMode::Incremental))
        {
            // 模板写坏了是常态（正在编辑），报错后继续监听，不能让服务器跟着退出。
            eprintln!("重新生成失败：{err}");
            continue;
        }
        println!("检测到 {changed} 处改动，已重新生成");
        // 生成成功才提版本号：写坏模板时不该把浏览器刷成 404 或旧页面
        server.bump();
    }
    Ok(())
}

fn cmd_deploy(project: &PathBuf, check_only: bool, build_first: bool) -> Result<()> {
    let mut builder = open(project)?;
    if build_first {
        builder.build(BuildMode::Incremental).context("生成失败")?;
    }

    let credentials = credentials_from_env(&builder.config)?;
    let deployer =
        staticsmith_deploy::from_config(&builder.config, credentials).context("发布配置无效")?;

    deployer.check().context("连接或凭证检查失败")?;
    if check_only {
        println!("检查通过");
        return Ok(());
    }

    let mut on_progress = |p: Progress| {
        if p.total > 0 {
            println!("[{}/{}] {}", p.current, p.total, p.message);
        } else {
            println!("{}", p.message);
        }
    };
    let report: DeployReport = deployer
        .deploy(&builder.paths.output, &mut on_progress)
        .context("发布失败")?;

    println!(
        "发布完成：{} → 上传 {} 个、跳过 {} 个，耗时 {} ms",
        report.target,
        report.uploaded.len(),
        report.skipped,
        report.duration_ms
    );
    if let Some(commit) = &report.commit {
        println!("提交：{commit}");
    }
    for warning in &report.warnings {
        println!("提示：{warning}");
    }
    Ok(())
}

fn cmd_check(project: &PathBuf) -> Result<()> {
    let builder = open(project)?;
    let templates = builder.templates().infos().count();
    let components = builder.templates().components().len();
    let pages = builder.pages().len();
    let drafts = builder.pages().iter().filter(|p| p.draft).count();
    // 非草稿但这次不会进产物的，就是「排着队等日期」的
    let scheduled = pages - drafts - builder.published_pages().len();

    println!("配置：{}", builder.paths.root.display());
    println!("模板 {templates} 个（其中全局组件 {components} 个）");
    println!("内容 {pages} 篇（草稿 {drafts} 篇）");
    if scheduled > 0 {
        println!("定时发布 {scheduled} 篇：日期未到，本次构建不会输出");
    }
    println!("检查通过");
    Ok(())
}

/// `audit` 的选项集合。参数比较多，单独成结构体免得函数签名读不出来。
#[derive(Debug, Clone, Copy)]
pub struct AuditOptions {
    pub seo: bool,
    pub links: bool,
    pub media: bool,
    pub build: bool,
    pub json: bool,
    pub fail_on: FailOn,
}

/// 体检：SEO 字段、站内死链、媒体资源。
///
/// 规则与桌面端「SEO」标签页、MCP 的 `audit_*` 完全同源——CI 里挡下来的问题，
/// 在界面里能看到一模一样的结论，不会出现「本地干净、CI 报错」。
///
/// 三项都不指定时全跑。死链体检读产物，所以给了 `--build` 让 CI 一步到位。
fn cmd_audit(project: &PathBuf, options: AuditOptions) -> Result<()> {
    let mut builder = open(project)?;
    if options.build {
        builder.build(BuildMode::Full).context("生成失败")?;
    }

    // 一个都没指定就是「全都要」，而不是「什么都不做」
    let all = !(options.seo || options.links || options.media);
    let seo = (all || options.seo).then(|| builder.audit_seo());
    let links = if all || options.links {
        Some(builder.audit_links().context("站内链接体检失败")?)
    } else {
        None
    };
    let media = if all || options.media {
        Some(builder.audit_media().context("媒体资源体检失败")?)
    } else {
        None
    };

    if options.json {
        let payload = serde_json::json!({
            "seo": seo,
            "links": links,
            "media": media,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_audit(seo.as_ref(), links.as_ref(), media.as_ref());
    }

    // 死链与破图归到「必须修」：它们是读者直接撞到的 404，不是风格建议。
    let broken = links.as_ref().map(|r| r.broken.len()).unwrap_or_default();
    let missing = media.as_ref().map(|r| r.missing.len()).unwrap_or_default();
    let unused = media.as_ref().map(|r| r.unused.len()).unwrap_or_default();
    let errors = seo.as_ref().map(|r| r.errors).unwrap_or_default() + broken + missing;
    let warnings = seo.as_ref().map(|r| r.warnings).unwrap_or_default();
    let hints = seo.as_ref().map(|r| r.hints).unwrap_or_default() + unused;

    let failed = match options.fail_on {
        FailOn::Never => 0,
        FailOn::Error => errors,
        FailOn::Warn => errors + warnings,
        FailOn::Hint => errors + warnings + hints,
    };
    if failed > 0 {
        bail!("体检未通过：{failed} 个问题达到 {:?} 门禁", options.fail_on);
    }
    if !options.json {
        println!("体检通过");
    }
    Ok(())
}

/// 人读的报告。JSON 留给脚本，这里只挑「拿到就能动手」的信息。
fn print_audit(
    seo: Option<&staticsmith_core::SeoReport>,
    links: Option<&staticsmith_core::LinkReport>,
    media: Option<&staticsmith_core::media::Report>,
) {
    if let Some(report) = seo {
        println!(
            "SEO：{} 分，检查 {} 页，必须修 {} / 建议修 {} / 可优化 {}",
            report.score, report.checked, report.errors, report.warnings, report.hints
        );
        for issue in &report.issues {
            let who = if issue.source.is_empty() {
                "站点"
            } else {
                &issue.source
            };
            println!("  [{}] {who} {}", issue.code, issue.message);
        }
    }

    if let Some(report) = links {
        if !report.built {
            println!("站内链接：还没有产物，先 staticsmith build（或加 --build）");
        } else {
            println!(
                "站内链接：{} 页 / 站内 {} 条 / 站外 {} 条，死链 {} 条",
                report.pages,
                report.internal,
                report.external,
                report.broken.len()
            );
            for link in &report.broken {
                println!("  {} ← {}", link.url, link.referenced_by.join("、"));
            }
        }
    }

    if let Some(report) = media {
        println!(
            "媒体资源：{} 个文件，未引用 {} 个（可回收 {} KB），破图 {} 处",
            report.total,
            report.unused.len(),
            report.reclaimable / 1024,
            report.missing.len()
        );
        for missing in &report.missing {
            println!("  {} ← {}", missing.url, missing.referenced_by.join("、"));
        }
    }
}

/// 启动 MCP 服务端。
///
/// stdio 是默认传输：客户端把本进程当子进程拉起，最省配置。
/// 走 stdio 时**不能**往 stdout 写任何非协议内容，因此提示信息一律走 stderr。
fn cmd_mcp(project: &PathBuf, sse: bool, port: u16, permissions: Permissions) -> Result<()> {
    // 先确认项目能打开，免得 Agent 连上来才发现路径错了。
    let _ = open(project)?;
    let server = McpServer::open(project, permissions).context("MCP 服务端初始化失败")?;

    if !sse {
        eprintln!("MCP（stdio）已就绪：{}", permissions.summary());
        return staticsmith_mcp::serve_stdio(&server).context("stdio 传输异常");
    }

    let http = staticsmith_mcp::serve_http(Arc::new(server), port)
        .context("MCP HTTP 服务器启动失败（端口可能被占用）")?;
    println!("MCP（HTTP）已就绪：{}", permissions.summary());
    println!("  Streamable HTTP : {}", http.mcp_endpoint());
    println!("  SSE（旧版传输） : {}", http.sse_endpoint());
    println!("按 Ctrl+C 停止");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn open(project: &PathBuf) -> Result<Builder> {
    if !scaffold::is_project(project) {
        bail!(
            "{} 不是 StaticSmith 项目（缺少 staticsmith.toml），可先执行 staticsmith init",
            project.display()
        );
    }
    Builder::open(project).with_context(|| format!("打开项目失败：{}", project.display()))
}

/// CLI 的凭证只来自环境变量，便于在 CI 中注入。实现在 `staticsmith-deploy`，
/// 与 MCP 服务端共用同一套规则，避免两处各写一份而慢慢分叉。
fn credentials_from_env(
    config: &staticsmith_core::SiteConfig,
) -> Result<staticsmith_deploy::Credentials> {
    staticsmith_deploy::credentials::from_env(config).map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_defaults_to_incremental_in_current_dir() {
        let cli = Cli::try_parse_from(["staticsmith", "build"]).unwrap();
        match cli.command {
            Command::Build { full, project } => {
                assert!(!full);
                assert_eq!(project.project, PathBuf::from("."));
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn new_defaults_to_posts_section_and_draft() {
        let cli = Cli::try_parse_from(["staticsmith", "new", "标题"]).unwrap();
        match cli.command {
            Command::New {
                title,
                section,
                publish,
                ..
            } => {
                assert_eq!(title, "标题");
                assert_eq!(section, "posts");
                assert!(!publish, "默认建为草稿");
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn project_flag_is_accepted_after_the_subcommand() {
        let cli = Cli::try_parse_from(["staticsmith", "build", "--project", "/tmp/site"]).unwrap();
        match cli.command {
            Command::Build { project, .. } => {
                assert_eq!(project.project, PathBuf::from("/tmp/site"));
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn unknown_subcommand_is_rejected() {
        assert!(Cli::try_parse_from(["staticsmith", "publish"]).is_err());
    }

    #[test]
    fn init_then_build_produces_a_site() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        cmd_init(&root, Some("CLI 测试站")).unwrap();
        assert!(root.join("staticsmith.toml").is_file());

        let source = cmd_new(&root, &NewContent::new("命令行新建").in_section("posts")).unwrap();
        assert_eq!(source, "posts/命令行新建.md");

        let report = cmd_build(&root, BuildMode::Full).unwrap();
        assert!(report.pages_rendered >= 4);
        assert!(root.join("dist/index.html").is_file());
        assert!(root.join("dist/sitemap.xml").is_file());

        // 草稿默认不发布。
        assert!(!root.join("dist/posts/命令行新建/index.html").exists());

        // 第二次增量构建没有待生成页面。
        let plan = cmd_plan(&root, BuildMode::Incremental).unwrap();
        assert!(plan.is_empty(), "{:?}", plan.pages);
    }

    #[test]
    fn commands_refuse_non_project_directories() {
        let dir = tempfile::tempdir().unwrap();
        let err = cmd_build(&dir.path().to_path_buf(), BuildMode::Full).unwrap_err();
        assert!(err.to_string().contains("不是 StaticSmith 项目"));
    }

    #[test]
    fn import_defaults_to_posts_and_writes_by_default() {
        let cli = Cli::try_parse_from(["staticsmith", "import", "./old-site/content"]).unwrap();
        match cli.command {
            Command::Import {
                dir,
                section,
                dry_run,
                json,
                ..
            } => {
                assert_eq!(dir, PathBuf::from("./old-site/content"));
                assert_eq!(section, "posts");
                assert!(!dry_run, "默认真导入，要干跑得显式 --dry-run");
                assert!(!json);
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn import_can_target_the_root_section() {
        let cli = Cli::try_parse_from([
            "staticsmith",
            "import",
            "./in",
            "--section",
            "",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Command::Import {
                section, dry_run, ..
            } => {
                assert_eq!(section, "", "留空即根目录");
                assert!(dry_run);
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn audit_defaults_to_all_checks_and_error_gate() {
        let cli = Cli::try_parse_from(["staticsmith", "audit"]).unwrap();
        match cli.command {
            Command::Audit {
                seo,
                links,
                media,
                build,
                json,
                fail_on,
                ..
            } => {
                assert!(!seo && !links && !media, "都不指定表示全跑");
                assert!(!build && !json);
                assert_eq!(fail_on, FailOn::Error, "默认只在必须修的问题上失败");
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn audit_fails_on_dead_links_and_can_be_told_not_to() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        cmd_init(&root, Some("门禁测试站")).unwrap();

        std::fs::write(
            root.join("content/posts/dead.md"),
            "+++\ntitle = \"有死链的文章\"\n+++\n\n[没了](/posts/nowhere/)\n",
        )
        .unwrap();

        // --build 让 CI 一步到位：死链体检读的是产物
        let options = AuditOptions {
            seo: false,
            links: true,
            media: false,
            build: true,
            json: false,
            fail_on: FailOn::Error,
        };
        let err = cmd_audit(&root, options).unwrap_err();
        assert!(err.to_string().contains("体检未通过"), "{err}");

        // --fail-on never 只报告，退出码仍是 0
        let report_only = AuditOptions {
            fail_on: FailOn::Never,
            build: false,
            ..options
        };
        cmd_audit(&root, report_only).unwrap();
    }

    #[test]
    fn mcp_defaults_to_stdio_and_read_only() {
        let cli = Cli::try_parse_from(["staticsmith", "mcp"]).unwrap();
        match cli.command {
            Command::Mcp {
                sse,
                allow_write,
                allow_deploy,
                port,
                ..
            } => {
                assert!(!sse, "默认走 stdio");
                assert!(!allow_write, "默认只读");
                assert!(!allow_deploy);
                assert_eq!(port, 5330);
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn mcp_permissions_are_opt_in() {
        let cli = Cli::try_parse_from([
            "staticsmith",
            "mcp",
            "--sse",
            "--port",
            "0",
            "--allow-write",
            "--allow-deploy",
        ])
        .unwrap();
        match cli.command {
            Command::Mcp {
                sse,
                port,
                allow_write,
                allow_deploy,
                ..
            } => {
                assert!(sse);
                assert_eq!(port, 0);
                assert!(allow_write);
                assert!(allow_deploy);
            }
            other => panic!("解析到了 {other:?}"),
        }
    }

    #[test]
    fn credentials_come_from_the_deploy_crate() {
        // 具体规则在 staticsmith-deploy::credentials 里有完整测试，这里只确认接线正确。
        let err = credentials_from_env(&staticsmith_core::SiteConfig::default()).unwrap_err();
        assert!(err.to_string().contains("deploy.type"));
    }
}
