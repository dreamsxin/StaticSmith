//! StaticSmith 命令行入口。
//!
//! 定位：桌面端负责可视化编辑，CLI 负责自动化——CI 里构建、服务器上发布、脚本里批量新建内容。
//! 两者共用 `staticsmith-core`，因此行为完全一致，不存在「界面能构建出来、CI 构建不出来」。
//!
//! 凭证策略与桌面端不同：CLI 不碰系统凭据管理器（CI 环境里没有），只读环境变量。

use std::path::PathBuf;
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

    /// 启动本地预览服务器（仅监听 127.0.0.1）
    Serve {
        /// 端口，0 表示由系统分配
        #[arg(short = 'P', long, default_value_t = 5321)]
        port: u16,
        /// 启动前先生成一次
        #[arg(long)]
        build: bool,
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
        Command::Serve {
            port,
            build,
            project,
        } => cmd_serve(&project.project, port, build),
        Command::Deploy {
            check_only,
            build,
            project,
        } => cmd_deploy(&project.project, check_only, build),
        Command::Check { project } => cmd_check(&project.project),
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

fn cmd_serve(project: &PathBuf, port: u16, build_first: bool) -> Result<()> {
    let builder = if build_first {
        let mut builder = open(project)?;
        builder.build(BuildMode::Incremental).context("生成失败")?;
        builder
    } else {
        open(project)?
    };

    let server = PreviewServer::start(&builder.paths.output, port)
        .context("预览服务器启动失败（端口可能被占用）")?;
    println!("预览地址：{}", server.base_url());
    println!("按 Ctrl+C 停止");

    // 服务器在后台线程里跑，主线程挂起等待 Ctrl+C（进程退出时线程随之结束）。
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
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

    println!("配置：{}", builder.paths.root.display());
    println!("模板 {templates} 个（其中全局组件 {components} 个）");
    println!("内容 {pages} 篇（草稿 {drafts} 篇）");
    println!("检查通过");
    Ok(())
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
