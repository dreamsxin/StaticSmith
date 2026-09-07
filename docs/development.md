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
```

`npm run tauri dev` 依赖 `dist-ui/`（`tauri.conf.json` 的 `frontendDist`）在生产构建时存在；
开发模式走 `devUrl`，所以首次开发不必先跑 `npm run build`，但 `cargo build -p staticsmith-app`
需要该目录存在。

## 代码组织约定

- `staticsmith-core` 不允许依赖 Tauri 或任何 GUI 能力。它要能被 CLI / 服务端直接复用
- 路径在跨平台边界统一转为正斜杠（`util::to_slash`）。模板名与索引键都依赖这一点，
  否则 Windows 与 macOS 的索引不兼容
- 新增 IPC 命令时同步三处：`commands.rs` 实现、`lib.rs` 的 `invoke_handler` 注册、
  `src/api.ts` 的类型声明
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
- 所有异步调用经 `run()` 包装，统一处理 busy 标记与错误消息
- 预览用 iframe + `srcdoc` + `sandbox="allow-same-origin"`，站点脚本不会影响宿主界面

## 尚未完成的方向

- 块级拖拽富文本编辑（当前是 Markdown 源文编辑）
- 主题包 `.zip` 导入导出
- i18n 宏库与多语言构建
- 基准测试（criterion）
