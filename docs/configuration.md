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
page_size = 10
minify = true

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
- `page_size`：列表页每页条数，必须大于 0
- `minify`：是否压缩输出 HTML。压缩只折叠标签间空白，`pre` / `code` / `script` /
  `style` / `textarea` 内部原样保留，不会破坏手写的 JS/CSS

相对路径按站点根目录解析（`ProjectPaths`）；绝对路径原样使用。

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
