//! 暴露给 Agent 的工具集。
//!
//! 权限分三级，默认只开放读取：Agent 误删内容或误发布线上站点的代价远高于少几个工具。
//! `tools/list` 只列出当前允许的工具——让 Agent 看到用不了的工具只会导致反复试错。

use serde_json::{json, Value};
use staticsmith_core::build::BuildMode;
use staticsmith_core::{Builder, NewContent};

use crate::protocol::tool_result;

/// 服务端允许 Agent 做到哪一步。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Permissions {
    /// 允许写内容、模板与生成产物。
    pub write: bool,
    /// 允许执行发布。
    pub deploy: bool,
}

impl Permissions {
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn allows(&self, access: Access) -> bool {
        match access {
            Access::Read => true,
            Access::Write => self.write,
            Access::Deploy => self.deploy,
        }
    }

    /// 供 `initialize` 的提示信息使用。
    pub fn summary(&self) -> String {
        match (self.write, self.deploy) {
            (false, false) => "只读模式".to_string(),
            (true, false) => "可读写内容与模板".to_string(),
            (true, true) => "可读写并发布".to_string(),
            (false, true) => "仅发布（未开放写入）".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Deploy,
}

/// 一个工具的元信息。
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub access: Access,
    /// JSON Schema，描述 `arguments` 结构。
    pub schema: fn() -> Value,
}

/// 全部工具定义。
pub fn all() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "site_info",
            title: "站点概览",
            description: "返回站点配置、目录结构、模板与内容统计、最近构建记录。先调它了解上下文。",
            access: Access::Read,
            schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        },
        ToolDef {
            name: "list_pages",
            title: "列出内容页",
            description: "列出全部内容页（source/title/url/template/section/draft/date）。可按栏目过滤。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "section": { "type": "string", "description": "只列这个栏目，空串表示根目录" },
                        "include_drafts": { "type": "boolean", "description": "是否包含草稿，默认 true" }
                    },
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "read_content",
            title: "读取内容原文",
            description: "按 source（相对 content/ 的路径，如 posts/hello.md）读取原文，含 +++ TOML front matter。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "source": { "type": "string" } },
                    "required": ["source"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "search_content",
            title: "搜索内容",
            description: "在标题与正文里做大小写不敏感的子串搜索，返回命中页面与片段。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "description": "最多返回条数，默认 20" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "list_templates",
            title: "列出模板",
            description: "列出模板及其角色（layout/component/page/partial）与直接依赖，用于理解级联关系。",
            access: Access::Read,
            schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        },
        ToolDef {
            name: "read_template",
            title: "读取模板",
            description: "按模板名（相对 templates/ 的路径，如 components/header.html）读取源码。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string" } },
                    "required": ["name"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "build_plan",
            title: "查看待生成范围",
            description: "只计算不写盘：待生成页面、变更模板、级联影响范围、将清理的产物。改模板前先看它。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "mode": { "type": "string", "enum": ["incremental", "full"] } },
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "create_content",
            title: "新建内容",
            description: "按标题生成 front matter 骨架，默认草稿。重名自动追加序号，不会覆盖已有文件。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "section": { "type": "string", "description": "栏目目录，默认 posts" },
                        "slug": { "type": "string" },
                        "template": { "type": "string" },
                        "description": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "draft": { "type": "boolean", "description": "默认 true" }
                    },
                    "required": ["title"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "write_content",
            title: "写入内容原文",
            description: "整文件覆盖写入（含 front matter），返回增量构建计划。写之前建议先 read_content。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "source": { "type": "string" },
                        "raw": { "type": "string", "description": "完整文件内容，含 +++ front matter" }
                    },
                    "required": ["source", "raw"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "audit_seo",
            title: "SEO 体检",
            description: "检查标题、描述、关键词、重复内容与站点级配置，返回按严重程度排序的问题清单。与桌面端「SEO 体检」用同一份规则。草稿不参与。",
            access: Access::Read,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "severity": {
                            "type": "string",
                            "enum": ["error", "warn", "hint"],
                            "description": "只看不低于该级别的问题"
                        },
                        "source": { "type": "string", "description": "只看某一篇内容" }
                    },
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "audit_media",
            title: "媒体资源体检",
            description: "列出没人引用的媒体文件（可回收空间）与引用了却不存在的地址（破图）。引用范围含内容、模板与主题。只读：删除文件交给人在界面里确认。",
            access: Access::Read,
            schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        },
        ToolDef {
            name: "patch_front_matter",
            title: "改写 front matter 字段",
            description: "只改指定字段（标题、描述、关键词、标签、日期、草稿开关），正文与未涉及的键、注释原样保留。给空串或空数组表示删除该键。补 SEO 字段用这个，不要用 write_content 整文覆盖。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "source": { "type": "string" },
                        "title": { "type": "string" },
                        "description": { "type": "string" },
                        "keywords": { "type": "array", "items": { "type": "string" } },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "date": { "type": "string", "description": "YYYY-MM-DD 或 RFC3339" },
                        "slug": { "type": "string" },
                        "template": { "type": "string" },
                        "draft": { "type": "boolean" },
                        "weight": { "type": "integer" }
                    },
                    "required": ["source"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "delete_content",
            title: "删除内容",
            description: "删除内容文件。下次生成时会清理它的产物。不可撤销。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "source": { "type": "string" } },
                    "required": ["source"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "write_template",
            title: "写入模板",
            description: "覆盖写入模板源码，返回级联影响范围。改全局组件会影响所有引用它的页面。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "source": { "type": "string" }
                    },
                    "required": ["name", "source"],
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "build_site",
            title: "生成站点",
            description: "生成产物到输出目录。默认智能增量，mode=full 为全量。",
            access: Access::Write,
            schema: || {
                json!({
                    "type": "object",
                    "properties": { "mode": { "type": "string", "enum": ["incremental", "full"] } },
                    "additionalProperties": false
                })
            },
        },
        ToolDef {
            name: "deploy_site",
            title: "发布站点",
            description: "按配置发布到 Git 或 FTP/SFTP。凭证取自环境变量。这一步会影响线上站点。",
            access: Access::Deploy,
            schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        },
    ]
}

/// 当前权限下可见的工具列表（MCP `tools/list` 的结果体）。
pub fn list_json(permissions: Permissions) -> Value {
    let tools: Vec<Value> = all()
        .into_iter()
        .filter(|t| permissions.allows(t.access))
        .map(|t| {
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": {
                    "readOnlyHint": t.access == Access::Read,
                    // 删除内容与发布是不可逆的，明确标出来，客户端会据此要求人工确认。
                    "destructiveHint": matches!(t.name, "delete_content" | "deploy_site"),
                    "idempotentHint": t.access == Access::Read
                }
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// 执行一个工具调用，返回 MCP `tools/call` 结果体。
pub fn call(builder: &mut Builder, permissions: Permissions, name: &str, args: &Value) -> Value {
    let Some(def) = all().into_iter().find(|t| t.name == name) else {
        return tool_result(format!("未知工具: {name}"), true);
    };
    if !permissions.allows(def.access) {
        return tool_result(
            format!(
                "工具 {name} 需要更高权限（当前：{}）。请以 --allow-write / --allow-deploy 启动服务端。",
                permissions.summary()
            ),
            true,
        );
    }

    match execute(builder, name, args) {
        Ok(text) => tool_result(text, false),
        Err(message) => tool_result(message, true),
    }
}

fn execute(builder: &mut Builder, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "site_info" => site_info(builder),
        "list_pages" => list_pages(builder, args),
        "read_content" => read_content(builder, args),
        "search_content" => search_content(builder, args),
        "list_templates" => list_templates(builder),
        "read_template" => read_template(builder, args),
        "build_plan" => build_plan(builder, args),
        "audit_seo" => audit_seo(builder, args),
        "audit_media" => audit_media(builder),
        "create_content" => create_content(builder, args),
        "write_content" => write_content(builder, args),
        "patch_front_matter" => patch_front_matter(builder, args),
        "delete_content" => delete_content(builder, args),
        "write_template" => write_template(builder, args),
        "build_site" => build_site(builder, args),
        "deploy_site" => deploy_site(builder),
        other => Err(format!("未知工具: {other}")),
    }
}

// ---------------------------------------------------------------- 读取类

fn site_info(builder: &Builder) -> Result<String, String> {
    let config = &builder.config;
    let pages = builder.pages();
    let info = json!({
        "site": {
            "title": config.site.title,
            "description": config.site.description,
            "base_url": config.site.base_url,
            "language": config.site.language
        },
        "paths": {
            "root": builder.paths.root,
            "content": builder.paths.content,
            "templates": builder.paths.templates,
            "static": builder.paths.static_dir,
            "assets": builder.paths.assets,
            "output": builder.paths.output
        },
        "build": {
            "page_size": config.build.page_size,
            "minify": config.build.minify,
            "generate_sitemap": config.build.generate_sitemap,
            "generate_feed": config.build.generate_feed
        },
        "counts": {
            "pages": pages.len(),
            "drafts": pages.iter().filter(|p| p.draft).count(),
            "templates": builder.templates().infos().count(),
            "components": builder.templates().components().len()
        },
        "recent_builds": builder.index().recent_builds(5).map_err(err)?,
        "deploy_type": format!("{:?}", config.deploy.r#type).to_lowercase()
    });
    pretty(&info)
}

fn list_pages(builder: &Builder, args: &Value) -> Result<String, String> {
    let section = args.get("section").and_then(Value::as_str);
    let include_drafts = args
        .get("include_drafts")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let pages: Vec<Value> = builder
        .pages()
        .iter()
        .filter(|p| section.is_none_or(|s| p.section == s))
        .filter(|p| include_drafts || !p.draft)
        .map(|p| {
            json!({
                "source": p.source,
                "title": p.title,
                "url": p.url,
                "template": p.template,
                "section": p.section,
                "is_index": p.is_index,
                "draft": p.draft,
                "tags": p.tags,
                "date": p.date.map(|d| d.format("%Y-%m-%d").to_string())
            })
        })
        .collect();
    pretty(&json!({ "count": pages.len(), "pages": pages }))
}

fn read_content(builder: &Builder, args: &Value) -> Result<String, String> {
    let source = require_str(args, "source")?;
    let path = staticsmith_core::content::resolve_source(&builder.paths.content, source);
    std::fs::read_to_string(&path).map_err(|e| format!("读取 {source} 失败: {e}"))
}

fn search_content(builder: &Builder, args: &Value) -> Result<String, String> {
    let query = require_str(args, "query")?.to_lowercase();
    if query.is_empty() {
        return Err("query 不能为空".to_string());
    }
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .max(1) as usize;

    let mut matches = Vec::new();
    for page in builder.pages() {
        let haystack = format!("{}\n{}", page.title, page.content).to_lowercase();
        if let Some(at) = haystack.find(&query) {
            let start = haystack[..at]
                .char_indices()
                .rev()
                .nth(40)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let snippet: String = haystack[start..].chars().take(160).collect();
            matches.push(json!({
                "source": page.source,
                "title": page.title,
                "url": page.url,
                "snippet": snippet
            }));
            if matches.len() >= limit {
                break;
            }
        }
    }
    pretty(&json!({ "count": matches.len(), "matches": matches }))
}

fn list_templates(builder: &Builder) -> Result<String, String> {
    let templates: Vec<Value> = builder
        .templates()
        .infos()
        .map(|i| {
            json!({
                "name": i.name,
                "kind": i.kind,
                "dependencies": i.dependencies
            })
        })
        .collect();
    pretty(&json!({ "count": templates.len(), "templates": templates }))
}

fn read_template(builder: &Builder, args: &Value) -> Result<String, String> {
    let name = require_str(args, "name")?;
    let info = builder
        .templates()
        .get(name)
        .ok_or_else(|| format!("模板不存在: {name}"))?;
    std::fs::read_to_string(&info.path).map_err(|e| format!("读取模板 {name} 失败: {e}"))
}

fn build_plan(builder: &Builder, args: &Value) -> Result<String, String> {
    let plan = builder.plan(parse_mode(args)).map_err(err)?;
    pretty(&json!(plan))
}

// ---------------------------------------------------------------- 写入类

fn create_content(builder: &mut Builder, args: &Value) -> Result<String, String> {
    let mut request = NewContent::new(require_str(args, "title")?);
    request.section = args
        .get("section")
        .and_then(Value::as_str)
        .unwrap_or("posts")
        .to_string();
    request.slug = args.get("slug").and_then(Value::as_str).map(str::to_string);
    request.template = args
        .get("template")
        .and_then(Value::as_str)
        .map(str::to_string);
    request.description = args
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    request.tags = args
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    request.draft = args.get("draft").and_then(Value::as_bool).unwrap_or(true);

    let source = builder.create_content(&request).map_err(err)?;
    pretty(&json!({ "created": source, "draft": request.draft }))
}

/// SEO 体检。可按严重程度或单篇过滤——Agent 通常一次只修一批同类问题。
fn audit_seo(builder: &Builder, args: &Value) -> Result<String, String> {
    use staticsmith_core::seo::Severity;

    let report = builder.audit_seo();
    let floor = match args.get("severity").and_then(Value::as_str) {
        None => None,
        Some("error") => Some(Severity::Error),
        Some("warn") => Some(Severity::Warn),
        Some("hint") => Some(Severity::Hint),
        Some(other) => return Err(format!("severity 只能是 error / warn / hint，收到 {other}")),
    };
    let source = args.get("source").and_then(Value::as_str);

    let issues: Vec<&staticsmith_core::SeoIssue> = report
        .issues
        .iter()
        // Severity 的排序是「越严重越小」，因此过滤条件是 <=
        .filter(|i| floor.is_none_or(|f| i.severity <= f))
        .filter(|i| source.is_none_or(|s| i.source == s))
        .collect();

    pretty(&json!({
        "checked": report.checked,
        "errors": report.errors,
        "warnings": report.warnings,
        "hints": report.hints,
        "score": report.score,
        "issues": issues,
        "next": "用 patch_front_matter 逐篇补齐字段；描述建议 40-160 字，标题不超过 60 字"
    }))
}

/// 媒体资源体检。只读——删文件这种不可逆操作不开给 Agent。
fn audit_media(builder: &Builder) -> Result<String, String> {
    let report = builder.audit_media().map_err(err)?;
    pretty(&json!({
        "total": report.total,
        "total_size": report.total_size,
        "reclaimable": report.reclaimable,
        "unused": report.unused,
        "missing": report.missing,
        "next": "未引用文件请在桌面端「SEO」标签页里确认后删除；破图请改内容或模板里的地址"
    }))
}

fn write_content(builder: &mut Builder, args: &Value) -> Result<String, String> {
    let source = require_str(args, "source")?.to_string();
    let raw = require_str(args, "raw")?;
    let path = staticsmith_core::content::resolve_source(&builder.paths.content, &source);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&path, raw).map_err(|e| format!("写入 {source} 失败: {e}"))?;
    builder.reload().map_err(err)?;

    let plan = builder.plan(BuildMode::Incremental).map_err(err)?;
    pretty(&json!({ "written": source, "plan": plan }))
}

/// 只改 front matter 的指定字段，正文与其他键原样保留。
///
/// SEO 补字段的正路：`write_content` 整文覆盖要求 Agent 先读全文再原样吐回来，
/// 一旦它顺手「优化」了正文，改动就超出了预期范围。
fn patch_front_matter(builder: &mut Builder, args: &Value) -> Result<String, String> {
    use staticsmith_core::frontmatter;

    let source = require_str(args, "source")?.to_string();
    let path = staticsmith_core::content::resolve_source(&builder.paths.content, &source);
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取 {source} 失败: {e}"))?;

    let mut patch = frontmatter::Patch::default();
    let mut touched: Vec<&str> = Vec::new();
    if let Some(value) = args.get("title").and_then(Value::as_str) {
        patch.title = Some(value.to_string());
        touched.push("title");
    }
    if let Some(value) = args.get("description").and_then(Value::as_str) {
        patch.description = Some(value.to_string());
        touched.push("description");
    }
    if let Some(value) = args.get("date").and_then(Value::as_str) {
        patch.date = Some(value.to_string());
        touched.push("date");
    }
    if let Some(value) = args.get("slug").and_then(Value::as_str) {
        patch.slug = Some(value.to_string());
        touched.push("slug");
    }
    if let Some(value) = args.get("template").and_then(Value::as_str) {
        patch.template = Some(value.to_string());
        touched.push("template");
    }
    if let Some(value) = args.get("keywords") {
        patch.keywords = Some(string_list(value, "keywords")?);
        touched.push("keywords");
    }
    if let Some(value) = args.get("tags") {
        patch.tags = Some(string_list(value, "tags")?);
        touched.push("tags");
    }
    if let Some(value) = args.get("draft").and_then(Value::as_bool) {
        patch.draft = Some(value);
        touched.push("draft");
    }
    if let Some(value) = args.get("weight").and_then(Value::as_i64) {
        patch.weight = Some(value);
        touched.push("weight");
    }
    if touched.is_empty() {
        return Err("没有给出任何要改的字段".to_string());
    }

    let updated = frontmatter::apply(&raw, &patch).map_err(err)?;
    std::fs::write(&path, &updated).map_err(|e| format!("写入 {source} 失败: {e}"))?;
    builder.reload().map_err(err)?;

    let plan = builder.plan(BuildMode::Incremental).map_err(err)?;
    pretty(&json!({
        "patched": source,
        "fields": touched,
        "front_matter": frontmatter::read(&updated).map_err(err)?,
        "plan": plan
    }))
}

/// JSON 数组 → 字符串数组。混进非字符串时明确报错，而不是静默丢掉。
fn string_list(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let array = value
        .as_array()
        .ok_or_else(|| format!("{field} 需要字符串数组"))?;
    array
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{field} 里出现了非字符串项"))
        })
        .collect()
}

fn delete_content(builder: &mut Builder, args: &Value) -> Result<String, String> {
    let source = require_str(args, "source")?.to_string();
    let path = staticsmith_core::content::resolve_source(&builder.paths.content, &source);
    std::fs::remove_file(&path).map_err(|e| format!("删除 {source} 失败: {e}"))?;
    builder.reload().map_err(err)?;
    pretty(&json!({ "deleted": source }))
}

fn write_template(builder: &mut Builder, args: &Value) -> Result<String, String> {
    let name = require_str(args, "name")?.to_string();
    let source = require_str(args, "source")?;
    // 模板名来自 Agent，必须按相对路径清洗，避免写到 templates/ 之外。
    let relative = staticsmith_core::util::sanitize_relative_dir(&name);
    if relative.is_empty() {
        return Err(format!("模板名无效: {name}"));
    }
    let path = relative
        .split('/')
        .fold(builder.paths.templates.clone(), |acc, s| acc.join(s));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&path, source).map_err(|e| format!("写入模板 {relative} 失败: {e}"))?;
    builder.reload().map_err(err)?;

    let plan = builder.plan(BuildMode::Incremental).map_err(err)?;
    pretty(&json!({ "written": relative, "plan": plan }))
}

fn build_site(builder: &mut Builder, args: &Value) -> Result<String, String> {
    let report = builder.build(parse_mode(args)).map_err(err)?;
    pretty(&json!(report))
}

fn deploy_site(builder: &mut Builder) -> Result<String, String> {
    let credentials =
        staticsmith_deploy::credentials::from_env(&builder.config).map_err(|e| e.to_string())?;
    let deployer =
        staticsmith_deploy::from_config(&builder.config, credentials).map_err(|e| e.to_string())?;
    deployer.check().map_err(|e| e.to_string())?;

    let mut log = Vec::new();
    let mut on_progress = |p: staticsmith_deploy::Progress| log.push(p.message);
    let report = deployer
        .deploy(&builder.paths.output, &mut on_progress)
        .map_err(|e| e.to_string())?;
    pretty(&json!({ "report": report, "log": log }))
}

// ---------------------------------------------------------------- 工具函数

fn parse_mode(args: &Value) -> BuildMode {
    match args.get("mode").and_then(Value::as_str) {
        Some("full") => BuildMode::Full,
        _ => BuildMode::Incremental,
    }
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("缺少必填参数 {key}"))
}

fn pretty<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))
}

fn err(e: staticsmith_core::Error) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 脚手架出一个真项目，用来跑「体检 → 补字段 → 再体检」这条运营主链路。
    fn project() -> (tempfile::TempDir, Builder) {
        let dir = tempfile::tempdir().unwrap();
        staticsmith_core::scaffold::init_project(dir.path(), Some("测试站")).unwrap();
        let builder = Builder::open(dir.path()).unwrap();
        (dir, builder)
    }

    #[test]
    fn audit_then_patch_closes_the_issue() {
        let (_dir, mut builder) = project();
        let perms = Permissions {
            write: true,
            deploy: false,
        };

        // 找一篇缺描述的内容
        let before = call(&mut builder, perms, "audit_seo", &json!({}));
        let text = before["content"][0]["text"].as_str().unwrap().to_string();
        let report: Value = serde_json::from_str(&text).unwrap();
        let missing = report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["code"] == "description.missing")
            .map(|i| i["source"].as_str().unwrap().to_string());

        let Some(source) = missing else {
            // 脚手架内容已经写全了描述，这条链路无需再测
            return;
        };

        let patched = call(
            &mut builder,
            perms,
            "patch_front_matter",
            &json!({
                "source": source,
                "description": "由 Agent 补写的描述，长度落在建议区间内，说明这篇文章讲了什么。",
                "keywords": ["静态站点", "内容运营"]
            }),
        );
        assert_eq!(patched["isError"], false, "{patched}");

        let after = call(
            &mut builder,
            perms,
            "audit_seo",
            &json!({ "source": source }),
        );
        let text = after["content"][0]["text"].as_str().unwrap().to_string();
        let report: Value = serde_json::from_str(&text).unwrap();
        let codes: Vec<&str> = report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["code"].as_str().unwrap())
            .collect();
        assert!(
            !codes.contains(&"description.missing"),
            "补完描述后不该再报缺失：{codes:?}"
        );
    }

    #[test]
    fn patch_front_matter_needs_at_least_one_field() {
        let (_dir, mut builder) = project();
        let source = builder.pages()[0].source.clone();
        let result = call(
            &mut builder,
            Permissions {
                write: true,
                deploy: false,
            },
            "patch_front_matter",
            &json!({ "source": source }),
        );
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn audit_seo_rejects_an_unknown_severity() {
        let (_dir, mut builder) = project();
        let result = call(
            &mut builder,
            Permissions::read_only(),
            "audit_seo",
            &json!({ "severity": "critical" }),
        );
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn read_only_permissions_hide_write_tools() {
        let list = list_json(Permissions::read_only());
        let names: Vec<&str> = list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();

        assert!(names.contains(&"list_pages"));
        assert!(names.contains(&"build_plan"));
        // 体检是只读的：运营先看清单，再决定要不要给写权限
        assert!(names.contains(&"audit_seo"));
        assert!(!names.contains(&"write_content"));
        assert!(!names.contains(&"patch_front_matter"));
        assert!(!names.contains(&"build_site"));
        assert!(!names.contains(&"deploy_site"));
    }

    #[test]
    fn write_permission_exposes_write_tools_but_not_deploy() {
        let list = list_json(Permissions {
            write: true,
            deploy: false,
        });
        let names: Vec<&str> = list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();

        assert!(names.contains(&"write_content"));
        assert!(names.contains(&"build_site"));
        assert!(!names.contains(&"deploy_site"));
    }

    #[test]
    fn destructive_tools_are_annotated() {
        let list = list_json(Permissions {
            write: true,
            deploy: true,
        });
        let tools = list["tools"].as_array().unwrap();
        let find = |name: &str| {
            tools
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("缺少工具 {name}"))
                .clone()
        };

        assert_eq!(find("list_pages")["annotations"]["readOnlyHint"], true);
        assert_eq!(
            find("delete_content")["annotations"]["destructiveHint"],
            true
        );
        assert_eq!(find("deploy_site")["annotations"]["destructiveHint"], true);
        assert_eq!(
            find("write_content")["annotations"]["destructiveHint"],
            false
        );
    }

    #[test]
    fn every_tool_has_an_object_schema() {
        for tool in all() {
            let schema = (tool.schema)();
            assert_eq!(
                schema["type"], "object",
                "{} 的 schema 不是 object",
                tool.name
            );
            assert!(
                schema["properties"].is_object(),
                "{} 缺少 properties",
                tool.name
            );
        }
    }

    #[test]
    fn permission_summary_is_human_readable() {
        assert_eq!(Permissions::read_only().summary(), "只读模式");
        assert_eq!(
            Permissions {
                write: true,
                deploy: true
            }
            .summary(),
            "可读写并发布"
        );
    }
}
