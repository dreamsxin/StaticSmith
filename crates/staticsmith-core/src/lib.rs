//! StaticSmith 静态生成核心。
//!
//! 模块划分：
//! - [`config`]：`staticsmith.toml` 的读写与校验
//! - [`content`]：Markdown + TOML front matter 解析
//! - [`templates`]：Tera 模板加载与依赖提取
//! - [`graph`]：模板依赖图（级联更新的基础）
//! - [`index`]：SQLite 索引（哈希、脏标记、构建历史、资源登记）
//! - [`assets`]：媒体资源的内容寻址存储
//! - [`build`]：全量 / 增量构建引擎
//! - [`watch`]：文件监听
//!
//! 典型用法：
//!
//! ```no_run
//! use staticsmith_core::{build::BuildMode, Builder};
//!
//! let mut builder = Builder::open("./my-site")?;
//! let plan = builder.plan(BuildMode::Incremental)?;
//! println!("将重新生成 {} / {} 个页面", plan.pages.len(), plan.total_pages);
//! let report = builder.build(BuildMode::Incremental)?;
//! println!("耗时 {} ms", report.duration_ms);
//! # Ok::<(), staticsmith_core::Error>(())
//! ```

pub mod assets;
pub mod build;
pub mod config;
pub mod content;
pub mod error;
pub mod feeds;
pub mod filters;
pub mod frontmatter;
pub mod graph;
pub mod index;
pub mod links;
pub mod media;
pub mod outputs;
pub mod scaffold;
pub mod sections;
pub mod seo;
pub mod taxonomy;
pub mod templates;
pub mod util;
pub mod watch;

#[cfg(feature = "serve")]
pub mod serve;

pub use assets::{AssetStore, SavedAsset};
pub use build::{BuildMode, BuildPlan, BuildReport, Builder, Pagination};
pub use config::{AssetNaming, Assets, ProjectPaths, SiteConfig, Taxonomy, CONFIG_FILE_NAME};
pub use content::{NewContent, Page};
pub use error::{Error, Result};
pub use graph::TemplateGraph;
pub use index::Index;
pub use links::{BrokenLink, Report as LinkReport};
pub use outputs::{OutputFile, OutputKind};
pub use sections::Section;
pub use seo::{Issue as SeoIssue, Report as SeoReport, Severity as SeoSeverity};
pub use templates::{TemplateKind, TemplateSet};

#[cfg(feature = "serve")]
pub use serve::PreviewServer;

/// 核心库版本，界面「关于」页展示用。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
