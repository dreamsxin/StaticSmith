# 命令行工具

桌面端负责可视化编辑，CLI 负责自动化：CI 里构建、服务器上发布、脚本里批量新建内容。
两者共用 `staticsmith-core`，行为完全一致，不会出现「界面里能生成、CI 里生成不出来」。

```bash
cargo build --release -p staticsmith-cli
# 产物：target/release/staticsmith
```

## 命令

```bash
staticsmith init [目录] --title "站点名"     # 创建骨架，已存在文件不覆盖
staticsmith new "标题" [--section posts] [--slug custom] [--template pages/post.html] [--publish]
staticsmith build [--full]                    # 默认智能增量
staticsmith plan  [--full]                    # 只算影响范围，不写文件
staticsmith serve [--port 5321] [--no-watch]  # 本地预览，仅监听 127.0.0.1

staticsmith deploy [--build] [--check-only]   # Git 或 FTP/SFTP 发布
staticsmith check                             # 校验配置、模板与内容
staticsmith import <目录> [--section posts] [--dry-run] [--json]
                                              # 导入 Hugo / Jekyll 的内容（YAML → TOML）
staticsmith audit [--seo] [--links] [--media] [--build] [--json] [--fail-on error|warn|hint|never]
                                              # SEO / 死链 / 媒体体检，可作 CI 门禁

staticsmith batch tags <源文件…> [--add a,b] [--remove c] [--json]
staticsmith batch draft <源文件…> [--publish] [--json]
staticsmith batch move <源文件…> --to <栏目> [--no-aliases] [--dry-run] [--json]
staticsmith batch delete <源文件…> [--yes] [--json]
                                              # 批量改内容，判断与桌面端 / MCP 同源
staticsmith replace --find <文字> [--to <文字>] [--in <源文件>…] [--ignore-case] [--yes] [--json]
                                              # 跨文件替换正文，默认只干跑


staticsmith theme export <out.zip> [--name 名字] [--version 1.0.0] [--author 谁] [--description 一句话]
staticsmith theme import <包.zip> [--dry-run] [--overwrite] [--json]
                                              # 主题包：打包外观 / 装到别的站点
staticsmith mcp [--sse] [--port 5330] [--allow-write] [--allow-deploy]
                                              # MCP 服务端，见 docs/mcp.md
```

除 `init` 外，所有命令接受 `--project <目录>` 指定站点根目录（默认当前目录）。

`new` 默认建为草稿（`draft = true`），加 `--publish` 才直接发布——写了一行就被推上线不是好默认值。

`serve` 默认监听 `content/`、`templates/`、`themes/`，改动后自动增量重新生成（去抖 300 ms），
启动时也会先生成一次，因此首屏就有内容。重建失败（比如模板正在改、语法暂时不完整）只打印错误，
服务器继续跑，改好后下一次保存就恢复。纯静态托管场景用 `--no-watch` 关掉监听，
此时若想先生成一次再服务，额外加 `--build`。

浏览器不用手动刷新：预览服务器给 HTML 响应注入了一小段轮询脚本，产物版本号一变就自己重载，
并把滚动位置带回来。注入只发生在**响应**里，`dist/` 下的文件一个字节都不会改——
发布出去的站点没有这段代码。

`audit` 与桌面端「体检」标签页、MCP 的 `audit_*` 用同一份规则，不会出现「本地干净、CI 报错」：

- 三个开关都不给就全跑；死链体检读产物，所以 CI 里通常写 `staticsmith audit --build`
- `--fail-on` 默认 `error`：只有必须修的问题（含死链与破图）才让 CI 变红。
  把建议项也算失败会天天红，红久了就没人看了。想只看报告用 `--fail-on never`
- `--json` 输出三份完整报告，便于脚本挑字段或存档对比

`import` 用来接手别家的站点：

```bash
staticsmith import ../old-blog/content --dry-run   # 只看每篇会变成什么样
staticsmith import ../old-blog/content --section posts
```

- 递归找 `.md` / `.markdown`，YAML front matter（`---`）转成 TOML（`+++`），
  已经是 TOML 的原样留用；**正文一个字节都不动**
- 转不了的字段（嵌套映射、`layout` 等）写成 TOML 注释留在文件里，并在报告里列成警告，
  不会悄悄丢；`--dry-run` 时同样能看到
- 目标已存在则跳过，绝不覆盖；`--section` 留空即导入到根目录，目录层级保留
- 顺手处理 Jekyll 约定：`2026-03-05-标题.md` 的日期进 front matter、文件名去掉前缀，
  `published: false` → `draft = true`，`2026-02-03 10:20:00 +0800` 归一成 RFC3339
  （没写时区的按 UTC 并给出警告）
- 导入完照例走一遍 `staticsmith check` 与 `staticsmith audit`

## 批量动作

这四件事以前只有桌面端能做，脚本化整理站点走不通（改一批 front matter 得自己解析
TOML，还得处理旧地址）。判断与桌面端、MCP 共用 `staticsmith_core::batch`，
所以「界面里这么改」和「脚本里这么改」结果一致。

```bash
# 给几篇加标签、去标签（原有顺序保留，新标签追加在后面）
staticsmith batch tags posts/a.md posts/b.md --add 运营,长文 --remove 草稿

# 成批发布或收回草稿
staticsmith batch draft posts/a.md --publish
staticsmith batch draft posts/b.md

# 搬栏目：默认补旧地址，先干跑看清单
staticsmith batch move posts/a.md posts/b.md --to notes --dry-run
staticsmith batch move posts/a.md posts/b.md --to notes

# 删除：默认只干跑，加 --yes 才真删
staticsmith batch delete posts/old.md
staticsmith batch delete posts/old.md --yes
```

三条默认值是刻意的：

- **搬动默认补旧地址**（`--no-aliases` 关掉），与桌面端一致——改 URL 不补旧地址
  等于把所有外部链接打断，而整理目录结构本来是件例行事。
- **删除默认只干跑**。CLI 里没有就地确认，脚本一跑源文件就没了，而删除没有回收站，
  所以真删必须显式 `--yes`。
- **加去标签与切草稿没有干跑**：反手就能改回来，多一步确认只是白点一下。

输出都可以 `--json`，跳过的条目一定带原因（不带原因的话，看的人分不清
「本来就这样」还是「程序没做」）。

## 跨文件查找替换

改一个称呼、换一个产品名、统一一个术语——以前只能逐篇点开改，
或者退到 `sed`（然后连 front matter 一起改坏）。

```bash
# 先干跑：列出哪几篇、共几处、每处前后长什么样
staticsmith replace --find 旧名 --to 新名

# 确认后落盘
staticsmith replace --find 旧名 --to 新名 --yes

# 只在几篇里改；忽略大小写；--to 省掉就是删掉这个词
staticsmith replace --find Meta --to 元 --in posts/a.md --in posts/b.md --ignore-case
staticsmith replace --find "（务必）" --yes
```

四条约束（判断在 `staticsmith_core::replace`，桌面端与 MCP 走同一份）：

- **只动正文，front matter 一个字节都不碰。** 在 TOML 区域做纯文本替换会撞上引号与
  转义，把 `"` 换掉就毁了整份 front matter。要改标题、标签这类字段用 `batch`
  或桌面端的属性面板，那条路是保序改写
- **纯文本，不支持正则。** 正则最容易「本想改一个词、实际扫掉半篇」，
  而且干跑看着正常、命中却在别处。需要正则的人手里有 `sed`
- **默认只干跑**，与删除同一条理由：正文替换没有撤销栈，改错一个词不会报错，
  只会安静地把内容改坏
- **逐篇独立**：一篇 front matter 坏了不影响其余，结果里说清哪篇为什么没改

干跑与执行走同一份判断，「预览说三处、执行改了五处」不会发生。
每篇最多列 5 行示例，命中更多时写明「另有 N 处未列出」——处数总是全的。


## 主题包

换外观此前只能手动拷目录，`templates/` 一份、`themes/<主题>/` 一份，漏一个就渲染失败。
`theme` 把两者收进一个 zip：

```bash
staticsmith theme export ../minimal.zip --name 极简 --version 1.0.0
staticsmith theme import ../minimal.zip --dry-run     # 会写哪些文件、哪些会被覆盖
staticsmith theme import ../minimal.zip --overwrite   # 替换自己改过的模板
```

包的结构固定为三块：`theme.toml`（名字、版本、作者、一句话说明）、
`templates/`、`theme/`（对应站点配置里的 `theme_dir`，装包时落到当前站点自己的目录名下）。

三条约定：

- **只碰外观**：`content/` 与 `static/` 不进包、也不会被写。文章与上传的图片是站点的，
  不是主题的；打进包里就意味着「装个主题顺手覆盖别人的文章」
- **默认不覆盖**：已存在的文件一律跳过并列出来，`--overwrite` 才替换。
  `--dry-run` 先看清单，带 `!` 的就是会被覆盖的那些
- **不信任包里的路径**：`..`、绝对路径、盘符、以及 `templates/` 与 `theme/`
  之外的条目一律拒绝（zip slip），拒绝原因会打印出来

装完要 `staticsmith build --full`：换外观等于所有页面的模板都变了。

## 凭证：只读环境变量


桌面端把凭证放在系统凭据管理器里，CI 环境没有那套东西，所以 CLI 只读环境变量：

- Git Token：`STATICSMITH_GIT_TOKEN`
- SSH 私钥口令：`STATICSMITH_SSH_PASSPHRASE`（私钥路径仍在配置里的 `ssh_key_path`）
- FTP 密码：配置里 `[deploy.ftp] password_env` 指定的变量名，默认 `FTP_PASSWORD`

缺变量时报错会直接写出要设哪个变量，而不是笼统的「认证失败」。

## CI 示例

GitHub Actions：

```yaml
name: build-and-deploy
on:
  push:
    branches: [main]

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --path crates/staticsmith-cli
      - run: staticsmith check --project ./site
      - run: staticsmith build --project ./site --full
      - run: staticsmith audit --project ./site
      - run: staticsmith deploy --project ./site
        env:
          STATICSMITH_GIT_TOKEN: ${{ secrets.DEPLOY_TOKEN }}
```

Linux 上编译需要 `libwebkit2gtk-4.1-dev` 的只有桌面端；CLI 只依赖 Rust 工具链，
CI 里不必装 GUI 依赖——这也是把 CLI 拆成独立 crate 的原因之一。

## 退出码

成功为 0，任何失败为非 0，错误信息写到 stderr（带上下文链），因此可以直接用于流水线判断。
日志级别用 `RUST_LOG` 控制，例如 `RUST_LOG=debug staticsmith build`。
