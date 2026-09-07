# 发布机制

两条通道共享同一前置条件：产物目录必须存在且非空。空目录会被直接拒绝
（`Error::EmptyOutput`），避免把空站点同步上去清空线上内容。

## Git

`staticsmith-deploy::git::GitDeployer` 基于 `git2`，不调用系统 Git 可执行文件。

流程：

1. 在 `dist/` 上 `Repository::open`，失败则 `init`。注意 `open` 不会向上查找，
   所以即使站点本身在一个 Git 仓库里，也不会误操作项目仓库
2. `set_head` 指向目标分支 → `add_all` → `write_tree`
3. 与上一次提交做 diff。没有差异且已有提交时跳过，报告里给出提示而不是产生空提交
4. 提交（`user.name` / `user.email` 缺失时回退到 `StaticSmith <staticsmith@localhost>`），
   然后 `checkout_head` 让工作区与提交一致
5. 强制推送 `+refs/heads/<branch>:refs/heads/<branch>`

产物分支表示「当前站点快照」，历史线性性没有意义，因此强制推送是预期行为，而不是补救手段。

提交信息按 Tera 一次性渲染，因此 `站点更新于 {{ now() }}` 会展开为时间戳。
模板写错时退回原文，不会中断发布。

## FTP / SFTP

差异同步逻辑与协议解耦：`RemoteFs` 抽象出「查状态 / 建目录 / 上传」三个原语，
`ftp::sync` 负责比对与进度推进。因此同步算法可以用内存假实现完整测试。

判定为需要上传的条件（`manifest::plan_sync`）：

- 远端缺失
- 大小不同
- 本地修改时间比远端新 2 秒以上（容忍 FAT/FTP 的 2 秒时间戳精度）

两侧时间戳有任一缺失时只比大小。

**远端多余文件不会被删除。** 静态站点目录经常混有手工上传的资源，
静默删除的代价高于留下少量陈旧文件。需要清理时手动处理。

远端状态通过逐个文件的 `SIZE` / `MDTM`（FTP）或 `stat`（SFTP）查询获得，
不解析 `LIST` 输出——各家 FTP 服务器的 LIST 格式差异过大。代价是文件数多时
往返次数较多。

## 凭证

凭证只以参数形式传入发布层，不写配置文件，也不在 deploy crate 内缓存。
`Credentials` 的 `Debug` 实现是手写的，只打印用户名，不会把密码写进日志。

桌面端解析顺序（`src-tauri/src/commands.rs::resolve_credentials`）：

1. 操作系统凭据管理器（服务名 `StaticSmith`）
   - Git：条目名 `git:<remote>`
   - FTP：条目名 `ftp:<username>@<host>`
2. FTP 额外回退到 `password_env` 指定的环境变量
3. 都没有则报错，提示去「发布」页保存凭证

写入凭据管理器由 `save_secret` 命令完成，对应 Windows Credential Manager、
macOS Keychain、Linux Secret Service。前端保存后立即清空输入框，
密钥不留在前端状态里。

Git 的 `auth_type = "ssh"` 时，凭据管理器里的条目被当作私钥口令（passphrase）使用，
私钥本身通过 `ssh_key_path` 指定。

## Feature 开关

`staticsmith-deploy` 的默认特性是 `["git", "ftp"]`。SFTP 需要显式开启：

```toml
staticsmith-deploy = { path = "...", features = ["sftp"] }
```

单独拆开是因为 `ssh2` 依赖 libssh2，在部分平台需要额外构建工具链。
未启用时选择 SFTP 会得到明确的配置错误，而不是运行期失败。
