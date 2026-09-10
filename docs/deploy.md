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

## FTP

**FTP 是明文协议**：用户名、密码与全部内容都以明文过网。在不受信的网络上传
（公共 Wi-Fi、共享办公网）请改用 Git 发布。**SFTP 暂不支持**，见本文末尾。

差异同步逻辑与协议解耦：`RemoteFs` 抽象出「查状态 / 建目录 / 上传」三个原语，
`ftp::sync` 负责比对与进度推进。因此同步算法可以用内存假实现完整测试。

判定为需要上传的条件（`manifest::plan_sync`）：

- 远端缺失
- 大小不同
- 本地修改时间比远端新 2 秒以上（容忍 FAT/FTP 的 2 秒时间戳精度）

两侧时间戳有任一缺失时只比大小。

**远端多余文件不会被删除。** 静态站点目录经常混有手工上传的资源，
静默删除的代价高于留下少量陈旧文件。需要清理时手动处理。

远端状态通过逐个文件的 `SIZE` / `MDTM` 查询获得，
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

条目名由 `account_for_config` 一处算出，解析凭证与界面查询问的是同一个名字
（界面走 `deploy_account` 命令，不自己拼）。**条目按发布目标分而不按站点分**：
两个站点推同一个 remote、或发到同一台主机的同一账号，会共用同一条凭据——
同一个目标本来就是同一份凭据。代价是「同一目标、两套凭据」时后保存的覆盖前一条，
界面上看不出发生了覆盖。


写入凭据管理器由 `save_secret` 命令完成，对应 Windows Credential Manager、
macOS Keychain、Linux Secret Service。前端保存后立即清空输入框，
密钥不留在前端状态里。

Git 的 `auth_type = "ssh"` 时，凭据管理器里的条目被当作私钥口令（passphrase）使用，
私钥本身通过 `ssh_key_path` 指定。

## 远端已存在同名文件时（FTP）

`[deploy.ftp].overwrite` 决定这一步，选项照 FileZilla 的「文件已存在时」那一栏：

- `size_or_newer`（默认）：大小不同或本地更新才传
- `always`：一律重传，不比对
- `newer`：只看时间
- `size`：只看大小
- `skip`：远端已有就不动，只补新文件

远端**没有**这个文件时一律上传，与规则无关——`skip` 说的是「不覆盖已有的」，
不是「不发新文件」。界面上在「设置 → 发布」选，「发布」页的发布目标里也会显示当前规则。

### 为什么让你选，而不是我们判断

FTP 能拿到的比对材料只有文件大小和 `MDTM`（修改时间），两者都可能不可靠：

- **服务器不支持 `MDTM`**：时间戳整个拿不到，「本地是否更新」这个问题无法回答。
  此时同长度的改动（改一个字、换一个日期）在默认规则下会被跳过。
- **服务器时钟快于本地**：远端时间永远显得「更新」，于是**所有文件被永久跳过**。
  报告里只写「跳过 N 个」，看起来跟「没有改动」一模一样，而线上一直是旧的。

这两种情况我们分不清「远端确实是新的」和「时间戳在骗人」，只有你知道自己那台服务器
什么样。撞上「明明改了却传不上去」时，把规则改成 `always`：慢，但一定对。

Git 发布不受这一条影响——它比对的是内容哈希，由 git 自己做。

## Feature 开关与 SFTP 现状

`staticsmith-deploy` 的默认特性是 `["git", "ftp"]`。**SFTP 暂不支持**：

- 界面上没有「使用 SFTP」开关——一个能勾、能存、只在点「发布」时才失败的开关
  比没有它更糟；
- `deploy.ftp.sftp` 字段还留着（写过它的老配置仍能解析），但 `SiteConfig::validate()`
  会在保存时就拦下 `true` 并说明原因，而不是等到发布那一步；
- 代码本身在 `ftp.rs` 的 `mod secure` 里，由 `sftp` feature 控制。CI 现在带
  `--all-features` 跑 clippy 与测试，所以它不会再腐烂成「从不编译」的死代码。

要启用得自己加特性，代价是把 `ssh2` / `libssh2-sys` 打进产物——libssh2 是 C 库，
部分平台还需要额外的构建工具链：

```toml
staticsmith-deploy = { path = "...", features = ["sftp"] }
```

需要加密通道又不想等这个开关的话，Git 发布走的是 HTTPS，现在就能用。
