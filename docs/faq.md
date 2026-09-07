# 常见问题

**手动修改 `templates/` 下的源文件，界面能识别吗？**

能。后端有文件监听服务（`watch.rs`），检测到变更后发出 `project://changed` 事件，
界面顶部出现提示。点击「刷新组件树」重新加载模板并重算影响范围。
监听不会自动覆盖你正在编辑器里编辑的内容。

**索引文件损坏或误删了怎么办？**

直接删除 `.staticsmith/index.db` 即可。索引只是缓存，下一次构建查不到任何页面记录，
会全部重建，等价于全量生成。

**为什么修改了模板，增量生成却没有覆盖某个页面？**

两种可能：

1. 模板用了动态 include（`{% include page.template %}`），无法静态分析依赖。
   这类情况需要手动「生成全站」
2. 页面用 front matter 的 `template` 指定了别的模板，而那个模板不在变更闭包内

**统一模板支持多语言（i18n）吗？**

`site.language` 会传给模板，可以据此切换文案。但内置的 i18n 宏库与多语言构建
（每种语言一套输出）尚未实现，目前需要自己在模板里做条件判断，或建多个站点项目。

**Tauri 应用如何保证本地数据安全？**

FTP 密码与 Git Token 通过 `keyring` 写入操作系统凭据管理器
（Windows Credential Manager / macOS Keychain / Linux Secret Service）。
`staticsmith.toml` 里只有环境变量名与私钥路径，配置文件泄露不会暴露密钥。
发布层的 `Credentials` 类型手写了 `Debug`，日志里不会出现密钥内容。

**我的网站有复杂的 JS 交互或特殊字体，会被破坏吗？**

不会。把外部资源引用放进 `layouts/base.html` 的 `{% block head %}` 或
`{% block scripts %}`，生成时原样保留。开启 `minify` 时压缩只折叠标签间空白，
`pre` / `code` / `script` / `style` / `textarea` 内部完全不动。

**分页地址能自定义吗？**

目前固定为 `<栏目>/page/N/`（第 1 页写在栏目根）。这个规则在
`build.rs::Pagination::output_path` 中实现，需要改可以在那里调整，
模板侧无需变动，因为地址由后端注入。

**可以只用生成引擎、不要桌面界面吗？**

可以。`staticsmith-core` 不依赖 Tauri 与任何 GUI 能力：

```rust
use staticsmith_core::{build::BuildMode, Builder};

let mut builder = Builder::open("./my-site")?;
let report = builder.build(BuildMode::Incremental)?;
```

**草稿会被发布出去吗？**

不会。`draft = true` 的页面既不进产物，也不出现在列表页的 `items` 与全站 `pages` 里。

**粘贴进来的图片存到哪了？能改吗？**

默认存到 `static/images/<哈希前两位>/<哈希>.<扩展名>`，页面里引用 `/images/...`。
存放位置由 `[build] static_dir` 与 `[assets] dir` 决定，命名方式、哈希长度、是否分片、
URL 前缀都可以在「设置」里改。完整说明见 [媒体资源与目录结构](assets.md)。

**为什么预览里的图片显示不出来？**

内存预览用 iframe + `srcdoc` 渲染，没有本地文件访问权限，`/images/...` 这类地址取不到文件。
在预览面板点「启动本地服务器」即可：它起一个只监听 127.0.0.1 的静态服务器指向产物目录，
预览与线上完全一致（需要先「生成」一次）。CLI 里对应 `staticsmith serve`。

**同一张图片粘贴多次会占多份空间吗？**

不会。文件名就是内容哈希，第二次粘贴会命中已有文件并直接复用，返回结果里 `deduplicated`
为 `true`。反过来，删掉文章不会自动删掉它引用过的图片——目前没有资源垃圾回收。
