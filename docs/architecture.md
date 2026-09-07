# 架构说明

## 分层

```
┌───────────────────────────────────────────────────────────────┐
│                     Tauri 应用外壳（src-tauri）                │
│  ┌──────────────────────┐        ┌─────────────────────────┐  │
│  │ 前端 UI（src/）      │  IPC   │ 命令层 commands.rs      │  │
│  │ Vue 3 + TypeScript   │◀─────▶│ 状态与监听 state.rs     │  │
│  └──────────────────────┘  事件  └───────────┬─────────────┘  │
└─────────────────────────────────────────────┼─────────────────┘
                                              │
        ┌─────────────────────────────────────┼───────────────┐
        │                                     │               │
┌───────┴────────────┐          ┌─────────────┴────────┐      │
│ staticsmith-cli    │          │ staticsmith-core     │      │
│ init/build/serve/  │─────────▶│ 配置 内容 模板 图    │      │
│ deploy/check       │          │ 资源 索引 构建 预览  │      │
└────────────────────┘          └───────────┬──────────┘      │
                                            │                 ▼
                                            │      ┌────────────────────────┐
                                            │      │ staticsmith-deploy     │
                                            │      │ Git · FTP · SFTP       │
                                            │      └───────────┬────────────┘
                                            ▼                  ▼
                        本地文件系统 + .staticsmith/index.db   Git 仓库 / FTP 服务器
```

桌面端与 CLI 是 `staticsmith-core` 的两个前端。core 不依赖 Tauri，也不依赖任何 GUI 能力，
因此 CI 里编译 CLI 不需要 WebView 相关的系统库。

## 关键设计

**依赖图与索引分工。** 依赖图是「模板之间」的关系（内存态，每次加载模板时重建）；
SQLite 索引保存「页面 ↔ 模板 ↔ 内容哈希」的持久映射。级联更新 = 图求闭包 + 索引查页面。

**渲染器与索引分离。** `Builder` 持有 SQLite 连接（`rusqlite::Connection` 不是 `Sync`），
因此并行渲染部分抽出无状态的 `Renderer`，只借用模板与配置，交给 Rayon 并行：

```rust
// crates/staticsmith-core/src/build.rs
let renderer = Renderer { templates: &self.templates, config: &self.config };
let rendered: Vec<Result<RenderedPage>> = selected
    .par_iter()
    .map(|page| renderer.render_page(page, &site_ctx, &all_pages))
    .collect();
```

**预览与产物同源。** `Builder::preview` 调用的是同一个 `Renderer::render_page`，
只是不写盘。预览与最终 HTML 不可能出现渲染差异。

**索引可丢弃。** `.staticsmith/index.db` 只是缓存。删除后下一次构建因为查不到任何页面记录
而全部重建，等价于全量生成。这让「索引损坏」不成为一类故障。

**资源内容寻址。** 媒体资源按内容哈希命名并写入 `static_dir` 之内，因此「写盘位置」与
「页面 URL」由同一份相对路径推导，不可能出现存进去了但页面 404 的情况。
详见 [媒体资源与目录结构](assets.md)。

## 增量构建判定

`Builder::plan` 对每个可发布页面依次检查（`build.rs::needs_rebuild`）：

1. 索引里没有记录 → 从未构建
2. `record.dirty` 为真，或源文件哈希变化 → 内容改了
3. 页面模板落在「变更模板的传递闭包」内 → 级联更新
4. 输出文件不存在 → 产物被手动删除

任一条命中即重新渲染。全量模式跳过检查，直接选中所有页面。

## 事件流

Rust → 前端的三个事件（`src-tauri/src/state.rs` 定义常量）：

- `project://changed`：文件监听器发现磁盘变更（外部编辑器改模板），界面提示刷新组件树
- `build://progress`：构建开始与结束
- `deploy://progress`：发布过程中每个文件的上传进度

文件监听在 300 ms 窗口内合并事件，避免一次保存触发多轮重建。

## 依赖选型

- **Tera**：Jinja2 风格模板，原生支持 `extends` / `include` / `macro`，与「统一模板」需求直接对应
- **rusqlite（bundled）**：内嵌 SQLite，不依赖系统库
- **Rayon**：数据并行渲染
- **git2**：不依赖系统 Git 可执行文件
- **suppaftp / ssh2**：FTP 与 SFTP，均为 feature 可选
- **keyring**：凭证交给操作系统凭据管理器保管

## 性能

产品文档中的性能数字（5000 篇 ~2.1s 等）是设计目标，尚未在本仓库做基准测试。
当前实现的性能特征是确定的：渲染按页面并行，增量模式下渲染量与「改动影响范围」成正比，
而不与站点规模成正比。要补基准测试可用 `cargo bench` 加 criterion，目前未引入。
