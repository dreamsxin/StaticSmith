# 配置说明

配置文件是站点根目录下的 `staticsmith.toml`。修改后可以在界面「设置」页保存，
也可以直接改文件——界面下一次刷新会读到。

## 完整示例

```toml
[site]
title = "我的 Rust 静态站"
description = "基于 Tauri 的可视化工具"
base_url = "https://example.com"
language = "zh-CN"

[build]
output_dir = "./dist"
content_dir = "./content"
theme_dir = "./themes/default"
template_dir = "./templates"
static_dir = "./static"
page_size = 10
minify = true
generate_sitemap = true
generate_feed = true
feed_limit = 20
publish_future = true  # false 即定时发布
source_format = "markdown"  # 或 "html"：正文原样输出


[assets]
dir = "images"          # 相对 static_dir
naming = "sha256"       # sha256 / md5 / original
hash_length = 16
shard = true
max_size_mb = 32
# url_prefix = "https://cdn.example.com/images"

[taxonomy]
enabled = true
slug = "tags"           # → /tags/ 与 /tags/<标签>/
title = "标签"
list_template = "pages/tags.html"
term_template = "pages/tag.html"

[[menu]]
name = "首页"
url = "/"
weight = 1

[[menu]]
name = "源码"
url = "https://github.com/user/repo"
weight = 9
blank = true

[deploy]
type = "git"          # none / git / ftp

[deploy.git]
remote = "https://github.com/user/repo.git"
branch = "main"
commit_message = "站点更新于 {{ now() }}"
auth_type = "token"   # token / ssh
# ssh_key_path = "~/.ssh/id_ed25519"

[deploy.ftp]
host = "ftp.example.com"
port = 21
username = "user"
password_env = "FTP_PASSWORD"
remote_path = "/public_html"
sftp = false                  # 暂不支持，必须为 false
```

## 字段说明

`[site]`

- `title`（必填）：站点标题，同时用于窗口标题与默认模板
- `description` / `base_url` / `language`：模板中通过 `site.*` 访问
- `extra`：任意扩展表，原样透传给 `site.extra`

`[build]`

- `output_dir`：产物目录，默认 `./dist`
- `content_dir`：Markdown 内容目录，默认 `./content`
- `theme_dir`：主题目录，其 `static/` 子目录会被复制到产物根
- `template_dir`：统一模板目录，默认 `./templates`
- `static_dir`：站点静态资源目录，默认 `./static`，整体复制到产物根
  （复制顺序在主题之后，因此同名文件由站点覆盖主题）
- `page_size`：列表页每页条数，必须大于 0
- `minify`：是否压缩输出 HTML。压缩只折叠标签间空白，`pre` / `code` / `script` /
  `style` / `textarea` 内部原样保留，不会破坏手写的 JS/CSS
- `generate_sitemap` / `generate_feed`：是否生成 `sitemap.xml` 与 `feed.xml`（Atom）。
  两者都需要 `site.base_url`；为空时跳过并在构建报告里给出提示
- `feed_limit`：订阅条目上限，默认 20，0 表示不限制
- `publish_future`：默认 `true`，未来日期的文章照常发布。改成 `false` 就是定时发布：
  `date` 晚于构建时刻的文章不进产物，也不出现在列表、标签页与订阅里；
  等构建时刻越过那个日期才出现。配合定时 CI 构建（例如每天一次）即「排好队，到点上线」。
  没写 `date` 的文章一律视为已到时间——把「忘了写日期」当成「永不发布」只会让人莫名少一篇。
- `source_format`：正文按哪种格式解析，默认 `"markdown"`。改成 `"html"` 就是**原样输出**：
  正文里写什么标签就是什么标签，`**` 只是两个星号，也不会被自动包 `<p>`。
  **全站统一，不支持每篇覆盖**：一个站点里两种正文格式混排，模板、体检、导入每一处都要
  先问「这一篇是哪种」，而收益只是省掉一次目录划分。想混写的人在 Markdown 模式下本来就
  可以直接写 HTML 块。
  刻意**不做**「HTML 里仍然跑一遍 Markdown」那种混合模式——那种模式下
  「这段为什么被转义了」永远解释不清。
  改这一项会改变**已有文章**的渲染结果，改完要做一次完整重建。
  `content/` 下的 `.html` / `.htm` 文件两种模式都会被收进来，切换格式不必给文件改名。
  桌面端在内容列表里给这类文章打「定时」徽标，并有同名筛选。


相对路径按站点根目录解析（`ProjectPaths`）；绝对路径原样使用。

`[assets]` —— 编辑器插入的图片等媒体资源，详见 [媒体资源与目录结构](assets.md)

- `dir`：相对 `static_dir` 的子目录，默认 `images`。不允许绝对路径或 `..`
- `naming`：`sha256`（默认）/ `md5` / `original`
- `hash_length`：哈希截断长度，8..=64，默认 16
- `shard`：是否用哈希前两位做二级目录，默认开启
- `max_size_mb`：单文件上限，默认 32，0 表示不限制
- `url_prefix`：URL 前缀覆盖。留空时按 `dir` 推导（`images` → `/images/`），
  填 CDN 地址可让页面直接引用 CDN

`[taxonomy]` —— 由 front matter 的 `tags` 生成标签页

- `enabled`：默认开启。关闭后既不生成标签页，也不会在文章页渲染标签链接
  （否则会留下指向不存在页面的死链）
- `slug`：URL 前缀，默认 `tags`，即 `/tags/` 与 `/tags/<标签>/`
- `title`：总览页标题，默认「标签」
- `list_template` / `term_template`：总览页与单标签页模板。
  模板缺失时跳过生成并在构建报告里给出提示，不会让整次构建失败

单标签页的分页沿用 `build.page_size`。标签 URL 保留中文原字（不转拼音）。

`[[taxonomies]]` —— 多个分类维度（标签 + 分类 + 自定义）

```toml
[[taxonomies]]
name = "tags"          # 读哪个 front matter 字段
slug = "tags"          # URL 前缀
title = "标签"
list_template = "pages/tags.html"
term_template = "pages/tag.html"

[[taxonomies]]
name = "categories"
slug = "categories"
title = "分类"
list_template = "pages/tags.html"
term_template = "pages/tag.html"
```

写了 `[[taxonomies]]` 就以它为准，单数的 `[taxonomy]` 不再生效——两套并存只会让人猜
哪个赢。字段与 `[taxonomy]` 相同，多出的 `name` 指定读哪个 front matter 字段
（留空按 `slug` 推）。两个维度用同一个 `slug` 会被校验拒绝，否则产物互相覆盖。

内容侧只需写对应字段：

```toml
tags = ["模板", "增量构建"]
categories = ["工程实践"]
```

没配成 taxonomy 的字段不生成页面，也不会在文章页渲染成链接（避免死链）。
模板侧用 `taxonomies` 遍历全部维度、`term_links.<字段名>` 取当前页面的链接，
见 [统一模板与级联更新](templates.md)。

设置界面把两种写法统一成一张「分类维度」列表：读配置时会把启用的单数 `[taxonomy]`
折进列表，保存时按数量还原——0 个写回禁用的 `[taxonomy]`，1 个写单数段，多个写
`[[taxonomies]]`。所以手写的配置不会被抹掉，也不会凭空多出一个空数组段。


`[[menu]]` —— 导航菜单

- `name`：菜单文字，必填
- `url`：目标地址，必填。站内以 `/` 开头（也允许 `#` 锚点），站外写完整 URL；
  相对地址会被校验拒绝——菜单是全站共用的，`posts/` 在不同深度的页面上指向不同目标
- `weight`：排序权重，小的在前。都不写就按书写顺序（排序是稳定的）
- `blank`：为真时新窗口打开，模板会补 `rel="noopener"`

菜单刻意做成配置而不是内容：它回答的是「这个站怎么被逛」，与某一篇文章无关。
模板只负责遍历 `menu`，所以加栏目改配置就够了，不用回头改 `components/header.html`。
分类维度不必在这里重复登记——脚手架模板另外遍历了 `taxonomies`，加一个维度自动出现在导航里。

不配 `[[menu]]` 的站点（升级上来的老站）导航保持原样：脚手架模板在 `menu` 为空时
退回内置的那几个链接。设置界面按顺序编辑菜单，保存时把行序写成 `weight = 1、2、3…`，
所以界面与手写配置看到的是同一个顺序。

地址里不能出现引号与尖括号：模板把 `url` 原样写进 `href`（不转义，否则 `/` 会变成
`&#x2F;`，浏览器认得但产物难看、死链体检也难读），所以这些字符在保存时就被拦下来。


`[deploy]`

- `type`：`none`（默认）/ `git` / `ftp`。选了 `git` 或 `ftp` 就必须提供同名配置段，
  否则保存时会被校验拒绝
- `[deploy.git]`：`remote`、`branch`、`commit_message`（支持 Tera 语法，如
  `{{ now() }}`）、`auth_type`、`ssh_key_path`
- `[deploy.ftp]`：`host`、`port`、`username`、`remote_path`、`password_env`、
  `sftp`（**暂不支持，必须为 `false`**；写成 `true` 会在保存时被校验拒绝。
  FTP 是明文协议，需要加密通道请改用 Git 发布，详见 [发布](deploy.md)）

## 设置页保存会保留你写的注释

界面保存设置走**保序改写**（`SiteConfig::merge_into`，与 front matter 同一套办法）：
只改动到的键，注释、键序、以及 StaticSmith 不认识的段（比如给别的工具看的
`[my-tool]`）都留在原处。以前是整文件重新序列化，保存一次注释就全没了。

一处例外：数组表（`[[taxonomies]]`、`[[menu]]`）整块替换，**它们内部的注释会丢**。
逐项对齐要先定义「哪一项是同一项」，而条目可以增删改序，猜错了比整块换掉更糟。
要在这两段里写注释，就手改配置文件、并避免在界面里改这两项。

## 密码与 Token 不写在这里

配置文件里只有 `password_env`（环境变量名）与 `ssh_key_path`（私钥路径），
没有任何密钥本身。凭证解析顺序见 [发布机制](deploy.md#凭证)。

## 内容 front matter

内容文件使用 `+++` 围栏包裹的 TOML front matter：

```markdown
+++
title = "统一模板与级联更新"
date = "2026-09-07"
description = "改一次头部，全站同步"
template = "pages/post.html"   # 可选，缺省按目录约定推导
slug = "cascading-updates"     # 可选，缺省取文件名
tags = ["模板", "增量构建"]
aliases = ["/posts/old-slug/"]  # 可选，旧地址，构建会生成重定向页
draft = false
weight = 0                     # 列表排序，越小越靠前
[extra]
cover = "/images/cover.png"
+++

正文……
```

没有 front matter 时整个文件都是正文，标题回退为第一个 `# ` 标题、否则文件名。
`date` 接受 `YYYY-MM-DD` 或 RFC3339。`draft = true` 的页面不会出现在产物与列表里。

`aliases` 是这篇文章的旧地址。改 slug、换栏目之后老链接就 404 了，把老地址写进
`aliases`，构建会为每一个写一张极小的重定向页：`meta refresh` 立刻跳到新地址、
`canonical` 指向新地址（让搜索引擎把权重并过去）、`noindex` 拦住旧地址自己被收录，
再留一条手点的链接兜住禁用跳转的环境。写法上 `/posts/old/` 与 `posts/old` 等价，
带扩展名的 `legacy.html` 按文件对待；指向自己的项会被忽略。

站内死链体检因此看不到这些旧地址——它们在产物里是真实存在的文件。
删除某个 alias 后旧的重定向页不会被自动清理（与分页产物同样的已知行为），
需要时做一次完整重建。

## 栏目

栏目不是配置项，而是 `content/` 下的一层目录：

- 目录里的 `index.md` / `_index.md` 是**栏目索引页**，它决定列表页（`pages/list.html`）。
  没有索引页的栏目，文章能访问，栏目地址本身是 404
- 文章模板按位置约定推导（栏目里的普通文件用 `pages/post.html`），新增栏目不需要新模板
- 列表页只收**同一层**的兄弟文章，不递归子栏目；分页由 `[build] page_size` 控制

界面里在内容侧栏管理栏目：新建（同时生成索引页）、改名（默认给每篇文章补 `aliases`）、
删除（仅空栏目）。Agent 侧对应 `list_sections` / `create_section` / `rename_section`，
删栏目不开给 Agent。日常动作与取舍见 [站点运营手册](operations.md)。


