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
cargo bench -p staticsmith-core # 构建性能基准（数字与解读见 architecture.md「性能」）
cargo fmt --all                 # 格式化
cargo clippy --workspace --all-targets

# CLI（不依赖 GUI 系统库）
cargo run -p staticsmith-cli -- --help
cargo run -p staticsmith-cli -- mcp --sse --port 0 --project ./site
```

`npm run tauri dev` 依赖 `dist-ui/`（`tauri.conf.json` 的 `frontendDist`）在生产构建时存在；
开发模式走 `devUrl`，所以首次开发不必先跑 `npm run build`，但 `cargo build -p staticsmith-app`
需要该目录存在。

**Tauri 的 npm 包与 Rust crate 必须落在同一 major.minor**，否则 `tauri dev` 一上来就报
`Found version mismatched Tauri packages` 并拒绝启动。要对齐的是四对：
`tauri` ↔ `@tauri-apps/api`、`tauri-plugin-dialog` ↔ `@tauri-apps/plugin-dialog`、
`tauri-plugin-opener` ↔ `@tauri-apps/plugin-opener`，以及跟着 `tauri` 走的 `@tauri-apps/cli`。

两边的更新节奏不同才是根因：Rust 侧写的是 `tauri = "2"` 这类宽松约束，`cargo update`
会把它带到新的次版本；npm 侧钉死精确版本，不会自己动。于是某天 `tauri dev` 突然起不来，
八成就是这个。对齐办法：`npm run tauri -- info` 打出两侧实际版本，照 Rust 侧改
`package.json` 里的钉版，再 `npm install`。

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
- FTP 同步测试用内存假远端（`RemoteFs` 实现），覆盖首次全量、二次跳过、
  单文件变更、远端多余文件保留（SFTP 那条路在 `sftp` feature 后面，CI 用
  `--all-features` 跑，见 [发布](deploy.md)）
- 前端用 vitest（`npm test`，配置在 `vitest.config.ts`）。当前覆盖两个着色器的
  **HTML 转义**与文本解析：着色结果经 `v-html` 插入，那是唯一的防线，
  却一直没有测试。断言的是「除了着色器自己的 `<span class="tok-*">`，
  输出里不该有别的标签」——逐个断言「这个 payload 被转义了」追不上新 payload。
  注意 payload 的**文字**该出现（源码视图本来就要显示源文），成为标签才是问题。

前端还没测到的部分：`store.ts` 的忙态计数器与未保存确认流（要先 mock
`@tauri-apps/api`）、`useSplit` 的夹取（要 jsdom）、组件渲染（要 `@vue/test-utils`）。
那三样各自需要新依赖，等真要测时再装，不提前付账。

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

## 变更日志与版本号

用户能感知的改动（新功能、行为变化、修掉的毛病）写进 [CHANGELOG.md](../CHANGELOG.md)
的「未发布」一节；纯内部重构与文档措辞不写——那些查 `git log` 更准。

发版时：把「未发布」改成版本号 + 日期，并同时改两处版本号——`Cargo.toml`（workspace）
与 `package.json`。两处不一致的话，桌面端显示的版本与 `staticsmith --version` 会互相打脸。

## 持续集成

`.github/workflows/ci.yml` 两个 job：

- **core**（Linux）：`cargo fmt --all --check` + clippy + 测试，只覆盖
  core / cli / deploy / mcp——它们不依赖 GUI 系统库，跑得快
- **desktop**（Windows）：`npm ci` → `npm run build`（tauri-build 需要 `dist-ui/` 存在）
  → `cargo test --workspace`，把桌面端连 WebView 一起编进去

`RUSTFLAGS: -D warnings` 让 clippy 与编译告警在 CI 里等同失败。

## 尚未完成的方向

按「运营者多久会撞一次」排序的完整清单（含每条为什么还没做）在
[站点运营手册](operations.md)，这里只留工程侧的摘要：

- 批量动作的更多干跑覆盖（搬动与删除已先预览再落盘，可逆动作暂不加这一步）
- 导入的界面入口与更多来源（命令行已支持 Hugo / Jekyll，WordPress XML 等还没认）
- 导航菜单的可视化编辑（现在导航写在 `components/header.html` 里）
- 栏目排序与元信息的编辑入口（索引页 front matter 已能写，界面还没有）
- 块级拖拽富文本编辑（当前是 Markdown 源文编辑）
- 资源垃圾回收的自动化：现在是显式体检 + 人工确认删除，没有「构建时自动清理」
- 浏览器端的局部热更新（现在是整页自动重载 + 保留滚动位置，不做 DOM 局部替换）
- 多窗口（一窗口一站点，事件定向已就绪）
- 主题包 `.zip` 导入导出
- i18n 宏库与多语言构建
- 基准测试（criterion）

