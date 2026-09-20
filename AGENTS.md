# AGENTS.md · 给 AI Agent 的施工说明书

这份文件给**要改这个仓库的 Agent** 看。人类读 [README](README.md) 与 [docs/](docs/)；
Agent 从这里开始，一页读完就能动手，需要细节时按第 9 节跳转。

不重复 [docs/development.md](docs/development.md) 的内容（环境、命令、组织约定在那里说得更全）。
这里只放三件事：**动手前必须知道的**、**会踩的坑**、**交付前必须过的关**。

## 1. 三十秒定位

Rust + Tauri 2 的桌面级静态网站生成工具。核心是**全局统一模板 + 组件级联更新**：
改一次头部，通过模板依赖图只重新生成受影响的页面。

同一套核心有**四个入口**，这是最重要的一条架构事实：

- 桌面端（`src-tauri` + `src/`，Vue 3）
- 命令行 `staticsmith`（`crates/staticsmith-cli`）
- MCP 服务端（`crates/staticsmith-mcp`，给别的 Agent 用，默认只读）
- 发布层（`crates/staticsmith-deploy`，Git / FTP / SFTP）

**一个能力做在核心里，三个入口都该能用到。** 只做桌面端的那一半算欠账（现在确实欠着，
见第 8 节）。反过来，`staticsmith-core` **不许依赖 Tauri 或任何 GUI 能力**。

## 2. 动手之前：验证套件

改完代码，这六条按顺序跑完再说「做好了」。它与 CI（`.github/workflows/ci.yml`）等价——
CI 带 `RUSTFLAGS: -D warnings`，所以 clippy 的**警告等于失败**。

```bash
cargo fmt --all                                     # 先格式化，再检查
cargo clippy --workspace --all-targets --all-features
cargo test --workspace --all-features
npx vitest run                                      # 前端 300+ 条
npx vue-tsc --noEmit                                # 类型检查
npm run build                                       # 打包（tauri-build 需要 dist-ui/ 存在）
```

`--all-features` 不是可选的：SFTP 那 120 行在 `sftp` feature 后面，不开这个开关它
从不编译、从不测试，而界面上有一个能勾的复选框。

只改前端时可以先跑后三条，但**提交前一定要六条全过**：`src-tauri/src/commands.rs`
与 `src/api.ts` 是同一个契约的两半，动一边常常牵动另一边。

## 3. 仓库地图

- `crates/staticsmith-core/` —— 生成引擎。`build.rs`（构建与级联）、`content.rs`（front matter
  与页面）、`sections.rs`（栏目 = `content/` 下的目录）、`history.rs`（内容快照，libgit2）、
  `skips.rs`（跳过项的统一措辞）、`taxonomy.rs`、`seo.rs`、`links.rs`、`media.rs`
- `crates/staticsmith-deploy/` —— 发布。`git.rs`、`ftp.rs`、`manifest.rs`（差异比对）、
  `credentials.rs`（凭证的环境变量规则只有这一份）
- `crates/staticsmith-mcp/` —— MCP 工具表与 stdio / HTTP 两条传输
- `crates/staticsmith-cli/` —— 命令行，`main.rs` 一个文件
- `src-tauri/` —— IPC。`commands.rs`（命令实现）、`state.rs`（会话与自身写盘登记）、
  `lib.rs`（`invoke_handler` 注册表）
- `src/` —— 前端。`store.ts`（唯一的状态与动作）、`api.ts`（IPC 类型声明）、
  `ui.ts`（布局、标签页、编辑器命令注册点）、`commands.ts`（菜单与命令面板的唯一命令表）、
  `grouping.ts` / `manuscript.ts` / `skips.ts` / `text.ts`（纯逻辑，好测的都在这里）、
  `components/`、`styles.css`（唯一的样式文件，`.vue` 里没有 `<style>`）

## 4. 改动落在哪里：同步点清单

漏掉任何一处的后果都是「功能看起来做了，实际上调不到」。

- **新增 IPC 命令**：`src-tauri/src/commands.rs` 实现 → `src-tauri/src/lib.rs` 的
  `invoke_handler` 注册 → `src/api.ts` 声明类型与包装函数 → `docs/ipc.md`
- **新增 MCP 工具**：`tools.rs` 的定义表 → 同文件的 `execute` 分派 → `docs/mcp.md`
- **新增菜单项 / 快捷键**：`src/commands.ts`（菜单栏与命令面板共用这一份）→ `docs/ui.md`
- **新增编辑器里的选区动作**：`src/ui.ts` 的 `EditorCommands` 接口 →
  `ContentEditor.vue` 的 `editorCommands` 实现（正文的改动只由编辑器写）
- **新增核心能力**：core 里实现 → `Builder` 上开方法 → 桌面端 / CLI / MCP 三个入口
  各接一遍（只接一个入口就要在收尾时明说欠着哪两个）
- **新增跳过 / 警告的措辞**：`staticsmith_core::skips`（Rust）与 `src/skips.ts`（前端）
  是一对孪生实现，两边各有测试钉同样的例子

## 5. 硬规矩

每条后面那句话是它的来历，不是装饰——知道为什么，才判断得了边界。

1. **每个写操作先干跑。** 会改多个文件、会改地址、会碰远端的动作，一律先返回
   「会发生什么」的清单，用户确认后才落盘（批量移动、删除、改 slug、栏目改名、发布、
   快照回退都走这条）。理由：这个工具没有跨文件撤销栈，Git 是唯一的安全网。
2. **一处措辞，一处定义。** 同一句话不许在界面、CLI、MCP 各写一遍。
   已经收敛的：跳过项摘要、阅读顺序（`content::reading_order` ↔ `grouping.ts` 的
   `readingOrder`）、字节数（`text.ts` 的 `formatBytes`）。
3. **判断与执行共用一份逻辑。** 「能不能挪」与「真的挪」调同一个函数，否则迟早
   出现「菜单能点、点了没反应」。
4. **错误一律带出错路径**（`Error::io(path, source)` 就是为此存在的）。
5. **路径在跨平台边界统一转正斜杠**（`util::to_slash`）。模板名与索引键都依赖这一点，
   否则 Windows 与 macOS 的索引不兼容。
6. **改 front matter 用 `toml_edit`**，不用 `toml` 往返：后者会抹掉顺序与注释，
   而那是用户手写的文件。
7. **停止 / 取消是合法结果，不是错误。** 发布可以中途停下，照常返回报告
   （`cancelled: true`），说清「传完了几个、线上此刻是什么状态」。
8. **界面上不出现实现细节的数字。** 顺序用「上移 / 下移」和拖拽表达，不摆
   `weight` 输入框。
9. **次要动作压暗（`--icon-idle`），不隐形；不可逆的动作永不压暗。**
   `opacity: 0` 曾让三颗按钮永久不可见却仍能点（`src/styles.test.ts` 现在守着这条）。
10. **一个符号只承担一件事。** `●` 是未保存，`↻` 是待重新生成——共用时「保存完圆点
    还亮着」会被读成「保存没生效」。

## 6. 测试要求

- **先写会失败的测试，再改代码。** 尤其是修 bug：没有先红过的测试，不算修好。
- 纯逻辑放进 `src/*.ts` 或 core 的模块里单独测；能抽成纯函数的判断就抽出来
  （几何、排序、插入位换算都这么处理过，理由是「留在组件里只能靠肉眼」）。
- **组件测试**用 `@vue/test-utils`，文件顶部写 `// @vitest-environment jsdom`——
  默认环境是 node，谁用 DOM 谁付启动开销。
- **样式表也有测试**（`src/styles.test.ts`）：组件测试在 jsdom 里根本不加载 CSS，
  类型检查管不到样式，所以「可见性」这类事故只能在那里拦。只钉有过真实事故的规则。
- 构建流程的端到端测试在 `crates/staticsmith-core/tests/build.rs`（临时目录里跑脚手架 →
  全量 → 改组件 → 验证级联范围 → 增量）。
- Git 发布测试用本地裸仓库当远端，**不 mock git2**；FTP 用内存假远端（`RemoteFs`）。

## 7. 交付物不只是代码

改了行为就要动文档，这在本仓库里是硬要求，不是加分项：

- 界面行为 → `docs/ui.md`；视觉与交互规范 → `docs/ui-design.md`
- IPC → `docs/ipc.md`；MCP → `docs/mcp.md`；命令行 → `docs/cli.md`
- 配置字段 → `docs/configuration.md`；发布 → `docs/deploy.md`；模板 → `docs/templates.md`
- 架构决策 → `docs/architecture.md`；日常动作与取舍 → `docs/operations.md`
- **每次面向用户的改动都要写 `CHANGELOG.md`**，写清「原先是什么样、为什么要改」，
  不是罗列改了哪些文件

文档的写法与代码注释同一条标准：**说为什么，不说是什么**。
「刻意不做的事」要写下来并给出理由，否则后人会当成缺陷去「补齐」。

提交信息用中文，首行 `type: 一句话说清做了什么`，正文用短横线列出关键决策与理由
（照着 `git log` 的样子写）。只有用户明确要求时才提交。

## 8. 已知的坑

会浪费你半小时的那些，按遇到概率排：

- **Tauri 的 npm 包与 Rust crate 必须同 major.minor**，否则 `tauri dev` 一上来就报
  `Found version mismatched Tauri packages`。根因是 Rust 侧 `tauri = "2"` 会被
  `cargo update` 带到新次版本，而 npm 侧钉死。对齐办法见 `docs/development.md`。
- **`import css from './styles.css?raw'` 在 vitest 里是空串**（Vitest 把所有 `.css`
  请求换成空串），断言会对着空字符串通过，护栏是假的。`src/styles.test.ts` 因此用
  `node:fs`，类型声明手写在 `src/vite-env.d.ts`（这个前端刻意没装 `@types/node`）。
- **`actions.setRaw()` 绕过 textarea 的原生撤销栈**：工具条与「整节挪动」都走它，
  所以那些动作按不了 `Ctrl+Z`。这是已知欠账，别在上面继续叠功能。
- **改文件时的锚点必须是文件里唯一的字符串**。这个仓库的注释密度很高，
  拿 `/**`、`function` 这类到处都有的片段当替换锚点，会一次改掉几十处
  （真发生过：`store.ts` 从 1604 行涨到 4935 行）。动 `store.ts`、`styles.css`、
  `PageList.vue` 这几个大文件前先确认锚点唯一。
- **推送常被 `Connection reset by ... port 22` 拦住**，等 20–90 秒重试即可，不是仓库问题。
- 界面**忙态是计数器不是布尔**（`store.busy`）：复合动作要在入口用 `busySpan()` 占一格，
  否则按钮会在一次点击里闪几次、`disabled` 提前解除。

## 9. 深入读什么

- 想懂整体设计与性能取舍：[docs/architecture.md](docs/architecture.md)
- 想懂界面为什么长这样（含「刻意不做」与欠账清单）：[docs/ui-design.md](docs/ui-design.md)
- 想懂界面每一块的行为：[docs/ui.md](docs/ui.md)
- 想懂用户拿它做什么：[docs/operations.md](docs/operations.md)、
  [docs/getting-started.md](docs/getting-started.md)
- 想懂开发环境与测试策略：[docs/development.md](docs/development.md)
