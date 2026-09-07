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

[assets]
dir = "images"          # 相对 static_dir
naming = "sha256"       # sha256 / md5 / original
hash_length = 16
shard = true
max_size_mb = 32
# url_prefix = "https://cdn.example.com/images"

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
sftp = false
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

相对路径按站点根目录解析（`ProjectPaths`）；绝对路径原样使用。

`[assets]` —— 编辑器插入的图片等媒体资源，详见 [媒体资源与目录结构](assets.md)

- `dir`：相对 `static_dir` 的子目录，默认 `images`。不允许绝对路径或 `..`
- `naming`：`sha256`（默认）/ `md5` / `original`
- `hash_length`：哈希截断长度，8..=64，默认 16
- `shard`：是否用哈希前两位做二级目录，默认开启
- `max_size_mb`：单文件上限，默认 32，0 表示不限制
- `url_prefix`：URL 前缀覆盖。留空时按 `dir` 推导（`images` → `/images/`），
  填 CDN 地址可让页面直接引用 CDN

`[deploy]`

- `type`：`none`（默认）/ `git` / `ftp`。选了 `git` 或 `ftp` 就必须提供同名配置段，
  否则保存时会被校验拒绝
- `[deploy.git]`：`remote`、`branch`、`commit_message`（支持 Tera 语法，如
  `{{ now() }}`）、`auth_type`、`ssh_key_path`
- `[deploy.ftp]`：`host`、`port`、`username`、`remote_path`、`password_env`、
  `sftp`（为真时走 SFTP）

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
draft = false
weight = 0                     # 列表排序，越小越靠前
[extra]
cover = "/images/cover.png"
+++

正文……
```

没有 front matter 时整个文件都是正文，标题回退为第一个 `# ` 标题、否则文件名。
`date` 接受 `YYYY-MM-DD` 或 RFC3339。`draft = true` 的页面不会出现在产物与列表里。
