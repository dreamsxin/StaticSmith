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
- `recent_projects() -> RecentEntry[]`：最近打开的站点，最新在前
- `forget_project(path) -> RecentEntry[]`：从最近列表移除（不动磁盘）

最近列表是唯一的**应用级**状态，存在 `app_config_dir()/recent.json`，上限 10 条，
读取时自动剔除已被删除或移动的站点。读写失败一律降级（空列表 / 只记日志），
不会让「打开项目」本身失败。

`ProjectSummary` 含配置、页面列表、布局、组件、全部模板、最近 10 次构建记录与脏页面列表。

## 配置

- `read_config() -> SiteConfig`
- `save_config(config) -> string[]`：校验后写回 `staticsmith.toml` 并重新打开项目

## 内容

- `list_pages() -> PageSummary[]`
- `read_content(source) -> string`：读取源文件原文（含 front matter）
- `create_content(request) -> string`：新建内容，返回其相对 `content/` 的路径
- `save_content({ source, raw }) -> BuildPlan`：写盘 + 重新解析 + 返回增量计划
- `delete_content(source) -> BuildPlan`
- `preview_page(source) -> string`：在父级布局下渲染完整 HTML，不写盘
- `read_front_matter(raw) -> FrontMatter`：读出字段供属性面板回填
- `apply_front_matter(raw, patch) -> string`：把属性改动折算成新的源文

后两个命令不看会话、不碰磁盘，传的是编辑器缓冲区里的文本——未保存的改动也能改属性。
`patch` 里省略的字段保持不动，空串与空数组表示删掉这个键（`description = ""` 与不写等价，
留着只会让源文变长）；`draft = false` 同理不写。正文、注释与未知键原样保留
（`crates/staticsmith-core/src/frontmatter.rs` 用 `toml_edit` 保序改写）。

`PageSummary.tags` 带上了页面标签，属性面板据此给出全站已用过的标签候选。


`create_content` 的 `request` 字段：`section`、`title`、`slug?`、`template?`、
`description?`、`tags?`、`draft?`（缺省 true）。文件名由 slug 或标题推导，
同名时追加 `-2`；`section` 里的 `..` 会被丢弃。

## 本地预览服务器

- `start_preview_server(port?) -> string`：启动并返回站点根地址，已启动时返回现有地址
- `stop_preview_server()`
- `preview_server_url() -> string | null`

服务器只监听 127.0.0.1，指向产物目录。内存预览（`preview_page`）取不到图片与 CSS，
因为 iframe 的 `srcdoc` 没有文件访问权限；需要完整效果时启动这个服务器。
它随项目关闭一起停止。

`source` 是相对 `content/` 的正斜杠路径，如 `posts/hello.md`。

## 媒体资源

- `save_asset(fileName, dataBase64) -> SavedAsset`：保存粘贴或拖入的文件
- `list_assets() -> AssetRecord[]`：已登记的资源清单

二进制走 base64 而不是字节数组：JSON IPC 传 `number[]` 会把每个字节膨胀成 2-4 个字符，
base64 只有 4/3 的开销。前端 `saveAsset(fileName, bytes)` 已封装编码细节。

`SavedAsset` 字段：`content_hash`（完整 SHA-256）、`file_name`、
`relative_path`（相对 `static_dir`）、`url`（可直接写进 Markdown）、`size`、
`deduplicated`（命中已有文件，未实际写盘）。

命名与去重规则见 [媒体资源与目录结构](assets.md)。

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
- `list_outputs() -> OutputFile[]`：扫描产物目录，还没生成过时返回空数组
- `audit_seo() -> SeoReport`：SEO 体检，读内存里的页面，不依赖产物

`SeoReport`：`checked`、`errors`、`warnings`、`hints`、`score`（0-100）与 `issues`。
每条 `issue` 带 `source`（站点级问题为空串）、`url`、`title`、`severity`
（`error` / `warn` / `hint`）、`code`（如 `description.missing`）与中文 `message`。
规则与阈值见 [SEO 与内容运营](seo.md)，与 MCP 的 `audit_seo` 同源。


`OutputFile`：`path`（相对产物目录）、`url`（站内地址）、`kind`、`size`。
`kind` 取 `page` / `pagination` / `taxonomy` / `sitemap` / `feed` / `asset`。
标签页与分页页没有源文件，内容树列不出来，界面只能靠这个清单给出入口；
它们也没法走内存预览，点击时前端会自动拉起本地预览服务器。


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

三个事件都用 `emit_to(window.label(), …)` 定向投递，而不是广播——多窗口下广播会让
A 站点的构建进度出现在 B 站点界面上。事件发送失败只记日志，不会中断业务流程。
