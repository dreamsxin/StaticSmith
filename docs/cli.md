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
staticsmith audit [--seo] [--links] [--media] [--build] [--json] [--fail-on error|warn|hint|never]
                                              # SEO / 死链 / 媒体体检，可作 CI 门禁
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

`audit` 与桌面端「SEO」标签页、MCP 的 `audit_*` 用同一份规则，不会出现「本地干净、CI 报错」：

- 三个开关都不给就全跑；死链体检读产物，所以 CI 里通常写 `staticsmith audit --build`
- `--fail-on` 默认 `error`：只有必须修的问题（含死链与破图）才让 CI 变红。
  把建议项也算失败会天天红，红久了就没人看了。想只看报告用 `--fail-on never`
- `--json` 输出三份完整报告，便于脚本挑字段或存档对比




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
