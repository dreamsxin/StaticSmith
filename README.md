# StaticSmith · 本地静站·工坊

> 可视化的自由，工业级的静态生成

基于 **Rust + Tauri 2** 的跨平台桌面级静态网站编辑工具。核心是 **全局统一模板（Global Layouts）**
与 **组件级联更新（Cascading Updates）**：改一次头部、底部或分页条，系统通过模板依赖图
定位所有受影响页面并只重新生成这些页面。

- 版本：v2.0.0
- 仓库：<https://github.com/dreamsxin/StaticSmith>
- 作者：dreamszhu &lt;dreamsxin@qq.com&gt;
- 文档：[docs/](docs/)

## 能力现状

已实现并有测试覆盖：

- 模板依赖图与级联影响分析（`extends` / `include` / `import` 静态解析）
- SQLite 索引驱动的全量 / 增量构建，产物清理，构建历史
- Markdown + TOML front matter 内容解析、pretty URL、草稿、栏目列表分页
- Tera 渲染、主题静态资源复制、可选 HTML 压缩
- 编辑器粘贴 / 拖入图片：按内容哈希（SHA-256 或 MD5）落盘到站点资源目录，自动去重
- 站点目录结构全部可配置（内容 / 模板 / 主题 / 静态资源 / 输出 / 资源子目录与 URL 前缀）
- Git 发布（`git2`，产物独立仓库 + 强制推送产物分支）
- FTP / SFTP 差异同步（按大小与修改时间比对，只传变化文件）
- 凭证写入操作系统凭据管理器（Windows Credential Manager / macOS Keychain / Secret Service）
- 桌面界面：内容编辑与布局继承预览、布局管理器、生成面板、发布面板、设置

尚未实现：块级拖拽式富文本编辑（当前编辑 Markdown 源文）、主题包 `.zip` 导入、i18n 宏库。

## 快速开始

前置：Rust stable、Node.js 20+、平台 WebView（Windows 10/11 自带 WebView2；Linux 需
`libwebkit2gtk-4.1-dev`）。

```bash
npm install                 # 安装前端依赖
npm run tauri dev           # 启动桌面端（自动拉起 Vite）
cargo test --workspace      # 运行 Rust 测试
npm run build               # 前端类型检查 + 打包
npm run tauri build         # 打包安装程序
```

首次运行界面后：**在空目录新建站点** → 自动写入 `templates/`、`themes/default/`、
`content/` 与 `staticsmith.toml` → 编辑内容 → 「生成」→「发布」。

## 仓库结构

```
StaticSmith/
├── Cargo.toml                    # Rust workspace（core / deploy / src-tauri）
├── package.json                  # 前端与 Tauri CLI 脚本
├── index.html  vite.config.ts    # 前端入口与构建配置
├── crates/
│   ├── staticsmith-core/         # 生成引擎（无 GUI 依赖，可独立复用）
│   │   ├── scaffold/             # 新建项目时写入的模板、主题与示例内容
│   │   ├── src/
│   │   │   ├── config.rs         # staticsmith.toml 读写与校验
│   │   │   ├── content.rs        # Markdown + front matter 解析
│   │   │   ├── templates.rs      # Tera 加载与依赖提取
│   │   │   ├── graph.rs          # 模板依赖图（级联更新基础）
│   │   │   ├── index.rs         # SQLite 索引
│   │   │   ├── assets.rs        # 媒体资源内容寻址存储
│   │   │   ├── build.rs         # 全量 / 增量构建引擎
│   │   │   ├── watch.rs          # 文件监听
│   │   │   ├── filters.rs        # 自定义 Tera 过滤器
│   │   │   └── scaffold.rs       # 新建项目
│   │   └── tests/build.rs        # 端到端构建测试
│   └── staticsmith-deploy/
│       └── src/
│           ├── manifest.rs       # 本地清单与差异同步计划
│           ├── git.rs            # Git 发布
│           └── ftp.rs            # FTP / SFTP 发布
├── src/                          # 前端（Vue 3 + TypeScript）
│   ├── api.ts                    # IPC 类型化封装（后端契约声明处）
│   ├── store.ts                  # 全局状态与动作
│   ├── App.vue
│   └── components/
├── src-tauri/                    # 桌面外壳
│   ├── src/commands.rs           # IPC 命令层
│   ├── src/state.rs              # 应用状态与文件监听
│   ├── tauri.conf.json
│   └── capabilities/default.json # 权限声明
└── docs/
```

## 站点项目结构

由「新建站点」生成，与本仓库无关：

```
my-site/
├── staticsmith.toml
├── content/                  # Markdown 内容源
│   ├── index.md
│   └── posts/{index.md,*.md}
├── templates/
│   ├── layouts/base.html     # 主布局
│   ├── components/           # 全局共享组件：header / footer / sidebar / pagination
│   └── pages/                # 页面模板：index / list / post
├── themes/default/static/    # 主题静态资源，构建时复制到输出目录
├── static/                   # 站点静态资源（含编辑器插入的图片，默认在 static/images/）
├── dist/                     # 生成产物
└── .staticsmith/index.db     # 本地索引（可安全删除，删除后退化为全量生成）
```

目录名全部来自 `staticsmith.toml`，可以按需改动，见 [媒体资源与目录结构](docs/assets.md)。

## 文档

- [架构说明](docs/architecture.md)
- [统一模板与级联更新](docs/templates.md)
- [媒体资源与目录结构](docs/assets.md)
- [配置说明](docs/configuration.md)
- [发布机制](docs/deploy.md)
- [IPC 命令参考](docs/ipc.md)
- [开发指引](docs/development.md)
- [常见问题](docs/faq.md)

## 许可

MIT，见 [LICENSE](LICENSE)。
