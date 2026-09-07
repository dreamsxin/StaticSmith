# IPC 命令参考

前端类型化封装在 `src/api.ts`，Rust 侧实现在 `src-tauri/src/commands.rs`。
字段命名与 Rust 结构体一致（serde 默认保留原名），改后端时两侧必须同步。

命令抛出的错误统一被序列化为一条中文字符串，前端 `invoke` 的 reject 分支直接可用。

## 项目生命周期

- `init_project(path, title?) -> PathBuf[]`：写入脚手架，返回创建的文件。已存在的文件跳过
- `is_project(path) -> bool`：目录下是否有 `staticsmith.toml`
- `open_project(path) -> ProjectSummary`：打开项目并启动文件监听
- `close_project()`：关闭项目，监听随之停止
- `project_summary() -> ProjectSummary`：重新读取当前项目快照

`ProjectSummary` 含配置、页面列表、布局、组件、全部模板、最近 10 次构建记录与脏页面列表。

## 配置

- `read_config() -> SiteConfig`
- `save_config(config) -> string[]`：校验后写回 `staticsmith.toml` 并重新打开项目

## 内容

- `list_pages() -> PageSummary[]`
- `read_content(source) -> string`：读取源文件原文（含 front matter）
- `save_content({ source, raw }) -> BuildPlan`：写盘 + 重新解析 + 返回增量计划
- `delete_content(source) -> BuildPlan`
- `preview_page(source) -> string`：在父级布局下渲染完整 HTML，不写盘

`source` 是相对 `content/` 的正斜杠路径，如 `posts/hello.md`。

## 模板

- `list_templates() -> TemplateInfo[]`
- `template_tree(root) -> TemplateNode`：布局 → 组件的继承树，成环节点标 `cyclic`
- `read_template(name) -> string`
- `save_template(name, source) -> BuildPlan`：返回级联影响范围

`name` 是相对 `templates/` 的正斜杠路径，如 `components/header.html`。

## 构建

- `build_plan(mode) -> BuildPlan`：只计算不写盘。`mode` 为 `"full"` 或 `"incremental"`
- `run_build(mode) -> BuildReport`
- `output_dir() -> PathBuf`：供前端用 opener 插件在文件管理器中打开

`BuildPlan` 关键字段：`pages`（待渲染源路径）、`changed_templates`、
`affected_templates`、`total_pages`、`orphaned_pages`（将被清理的产物）。

`BuildReport`：`pages_rendered`、`files_written`（含分页，故可能大于前者）、
`assets_copied`、`removed_files`、`duration_ms`、`warnings`。

## 发布

- `deploy_site() -> DeployReport`
- `check_deploy()`：只检查连接与凭证
- `save_secret(account, secret)` / `has_secret(account) -> bool` / `delete_secret(account)`

`account` 的命名规则由 Rust 侧 `account_for_git` / `account_for_ftp` 定义，
前端 `DeployPanel.vue` 必须使用同样的规则：

- Git：`git:<remote>`
- FTP：`ftp:<username>@<host>`

## 事件

- `project://changed`：载荷 `ChangeSet { templates, content, other }`（绝对路径数组）
- `build://progress`：载荷 `BuildProgress { phase, current, total }`
- `deploy://progress`：载荷 `Progress { message, current, total }`

事件发送失败只记日志，不会中断业务流程。
