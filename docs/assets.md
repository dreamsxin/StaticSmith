# 媒体资源与目录结构

## 插入图片的流程

在编辑器里粘贴（Ctrl/Cmd+V）或拖入文件，会发生这些事：

1. 前端把文件读成字节，base64 编码后交给 `save_asset` 命令
2. Rust 侧计算内容哈希，按命名策略生成文件名，写入 `static_dir/<assets.dir>/`
3. 文件登记进 SQLite 的 `assets` 表（主键是内容哈希）
4. 返回站内地址，编辑器把 `![alt](地址)` 插到光标处

构建时 `static_dir` 整体复制到产物根目录，所以资源与页面一起进入 `dist/`，
也一起被 Git / FTP 发布出去。

## 为什么按内容哈希命名

- **天然去重**：同一张图在多篇文章里粘贴多次，磁盘上只有一份，索引里也只有一条记录
- **可永久缓存**：内容变了文件名就变，CDN 与浏览器可以按不可变资源处理
- **无冲突**：剪贴板给的文件名通常是 `image.png`，按原名保存会互相覆盖

默认 SHA-256 截断到 16 位十六进制，并用前两位做二级目录：

```
static/images/3f/3fa1c2d4e5b60789.png
```

分片是为了避免单目录堆积上万文件——大多数文件系统在这种情况下会明显变慢。

## 命名策略

- `sha256`（默认）：内容哈希
- `md5`：同样是内容哈希，只是算法换成 MD5。提供它是为了和既有站点的资源命名保持一致，
  不要把它当安全手段用
- `original`：保留清洗后的原文件名（小写、非字母数字折叠为 `-`）。
  同名但内容不同时自动追加短哈希，例如 `cover-image-3fa1c2d4.png`

不管选哪种，索引里的 `content_hash` 始终是完整 SHA-256，去重逻辑不受命名策略影响。

## 目录结构可以怎么改

站点的每一层目录都由 `staticsmith.toml` 决定，不存在硬编码路径：

```toml
[build]
content_dir  = "./content"          # Markdown 内容源
template_dir = "./templates"        # 统一模板
theme_dir    = "./themes/default"   # 主题（其 static/ 子目录也会被复制）
static_dir   = "./static"           # 站点静态资源
output_dir   = "./dist"             # 产物

[assets]
dir = "images"                      # 相对 static_dir
```

想把资源放到 `public/media/2026/`、URL 变成 `/media/2026/...`：

```toml
[build]
static_dir = "./public"

[assets]
dir = "media/2026"
```

**`assets.dir` 只能是相对 `static_dir` 的路径**，这是刻意的约束：
写入位置与页面里的 URL 由同一份相对路径推导，因此不可能出现「文件存进去了但页面 404」。
绝对路径与包含 `..` 的路径会在保存配置时被拒绝。

需要走 CDN 时用 `url_prefix` 覆盖地址，文件仍然写在本地目录，由发布流程同步上去：

```toml
[assets]
dir = "images"
url_prefix = "https://cdn.example.com/images"
```

主题资源与站点资源的复制顺序是「先主题、后站点」，因此站点 `static_dir` 里的同名文件
会覆盖主题自带的资源——这是覆盖主题图片、字体的方式。

## 安全边界

用户给的文件名是外部输入，只被用来做两件事：

- 取扩展名：只保留 ASCII 字母数字，最长 8 位；取不到时按魔术字节嗅探
  （png / jpg / gif / webp / bmp / svg / pdf），仍然识别不出就用 `.bin`
- `original` 策略下作为文件主干：路径分隔符与 `..` 在这一步全部被丢掉

因此 `../../../etc/passwd.png` 只会变成资源目录里的 `passwd.png`。

单文件大小上限由 `assets.max_size_mb` 控制（默认 32 MB），超限在写盘之前就被拒绝。

## 已知限制

- 内存预览（iframe `srcdoc`）没有本地文件访问权限，图片显示为空占位。
  在预览面板点「启动本地服务器」或用 `staticsmith serve` 可获得与线上一致的效果
- 目前没有资源垃圾回收：删掉文章不会删掉它引用过的图片。
  索引里有完整资源清单（`list_assets`），后续可以据此做「查找未被引用的资源」
