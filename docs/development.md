# 开发指引

## 环境

- Rust stable（`rust-toolchain.toml` 已声明，含 rustfmt 与 clippy）
- Node.js 20+
- WebView：Windows 10/11 自带 WebView2；macOS 自带 WKWebView；
  Linux 需 `libwebkit2gtk-4.1-dev`、`build-essential`、`libssl-dev`

## 常用命令

```bash
npm install                     # 前端依赖
npm run tauri dev               # 桌面端开发（自动拉起 Vite dev server）
npm run build                   # vue-tsc 类型检查 + Vite 打包到 dist-ui/
npm run tauri build             # 打包安装程序

cargo test --workspace          # 全部 Rust 测试
cargo test -p staticsmith-core  # 只测生成引擎
cargo fmt --all                 # 格式化
cargo clippy --workspace --all-targets

# CLI（不依赖 GUI 系统库）
cargo run -p staticsmith-cli -- --help
cargo run -p staticsmith-cli -- mcp --sse --port 0 --project ./site
```

`npm run tauri dev` 依赖 `dist-ui/`（`tauri.conf.json` 的 `frontendDist`）在生产构建时存在；
开发模式走 `devUrl`，所以首次开发不必先跑 `npm run build`，但 `cargo build -p staticsmith-app`
需要该目录存在。

## 代码组织约定

- `staticsmith-core` 不允许依赖 Tauri 或任何 GUI 能力。它要能被 CLI / MCP / 服务端直接复用
- 路径在跨平台边界统一转为正斜杠（`util::to_slash`）。模板名与索引键都依赖这一点，
  否则 Windows 与 macOS 的索引不兼容
- 新增 IPC 命令时同步三处：`commands.rs` 实现、`lib.rs` 的 `invoke_handler` 注册、
  `src/api.ts` 的类型声明
- 新增 MCP 工具时同步两处：`tools.rs` 的定义表与 `execute` 分派
- 发布凭证的环境变量规则只有一份，在 `staticsmith-deploy::credentials`，CLI 与 MCP 共用
- 错误一律带上出错路径。`Error::io(path, source)` 就是为此存在的

## 测试策略

- 纯逻辑（依赖图、分页、清单比对、front matter 解析）走单元测试，快且无 IO
- 构建流程走 `crates/staticsmith-core/tests/build.rs` 的端到端测试：
  在临时目录跑脚手架 → 全量生成 → 改组件 → 验证级联范围 → 增量生成
- Git 发布测试用本地裸仓库当远端，真实跑完 commit + push，不 mock git2
- FTP/SFTP 同步测试用内存假远端（`RemoteFs` 实现），覆盖首次全量、二次跳过、
  单文件变更、远端多余文件保留

新增行为时优先在这些既有位置扩展，而不是新建测试文件。

## 前端约定

- 状态集中在 `src/store.ts`（`reactive` 单例 + `readonly` 导出）。组件只读状态、调用 actions
- 所有异步调用经 `run()` 包装，统一处理 busy 标记与错误；**失败一律弹通知**——
  只写进 `store.error` 而调用点不显示它，用户看到的就是「点了没反应」
- 涉及系统能力的调用（打开浏览器、定位文件）也走 store，这样权限被拒时能看到原因
- 预览用 iframe：内存预览走 `srcdoc`，本地服务器模式走 `src`（`sandbox` 只在前者加，
  否则会挡掉 http 源的加载）

## Tauri 权限的坑

`opener:allow-open-url` 的字面含义是「允许调用 open_url 命令，但没有任何预置 scope」——
URL 白名单为空，于是每次调用都被拒绝，前端如果没接错误处理就表现为「按钮没反应」。
要打开 http/https 链接得用 `opener:default`（它同时带上 `allow-default-urls` 的 scope
与 `allow-reveal-item-in-dir`）。

结论：加权限时不要只看命令名，要确认对应的 scope 也给了。前端调用一定要有错误出口。

## 持续集成

`.github/workflows/ci.yml` 两个 job：

- **core**（Linux）：`cargo fmt --all --check` + clippy + 测试，只覆盖
  core / cli / deploy / mcp——它们不依赖 GUI 系统库，跑得快
- **desktop**（Windows）：`npm ci` → `npm run build`（tauri-build 需要 `dist-ui/` 存在）
  → `cargo test --workspace`，把桌面端连 WebView 一起编进去

`RUSTFLAGS: -D warnings` 让 clippy 与编译告警在 CI 里等同失败。

## 尚未完成的方向

- 块级拖拽富文本编辑（当前是 Markdown 源文编辑）



- 资源垃圾回收的自动化：现在是显式体检 + 人工确认删除，没有「构建时自动清理」

- 浏览器端的局部热更新（现在是「保存即生成」+ iframe 整页重载）
- 多窗口（一窗口一站点，事件定向已就绪）
- 主题包 `.zip` 导入导出
- i18n 宏库与多语言构建
- 基准测试（criterion）
