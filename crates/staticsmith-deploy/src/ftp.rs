//! FTP / SFTP 发布：`rsync` 风格差异同步。
//!
//! 同步算法与协议解耦：[`RemoteFs`] 抽象出「查状态 / 建目录 / 上传」三个原语，
//! [`sync`] 负责比对与推进进度，因此可以用内存假实现完整测试同步逻辑。

use std::path::Path;
use std::time::{Duration, Instant};

use staticsmith_core::config::FtpOverwrite;

use crate::manifest::{self, RemoteEntry};
use crate::{
    ensure_output_ready, DeployPlan, DeployReport, Error, Flow, Progress, ProgressFn, Result,
};

/// 单个文件的上传尝试次数。
///
/// 上传是逐文件的，一次连接重置以前直接冒泡成失败——而那一刻线上已经是
/// 「一半新一半旧」。3 次足以吃掉一次瞬时抖动，服务器真的拒绝时也不会把
/// 等待拖长几倍。
const UPLOAD_ATTEMPTS: usize = 3;

/// 两次尝试之间等多久。
///
/// 线性而不是指数退避：这里挡的是「网线抖了一下」，不是被限流。
/// 指数等待只会让注定失败的那次发布更慢。
const RETRY_PAUSE: Duration = Duration::from_millis(300);

/// 远端文件系统原语。
pub trait RemoteFs {
    /// 查询远端文件状态；不存在返回 `Ok(None)`。
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>>;
    /// 创建目录，已存在时不应报错。
    fn mkdir(&mut self, path: &str) -> Result<()>;
    /// 上传单个文件。
    fn upload(&mut self, local: &Path, remote: &str) -> Result<()>;
    /// 重新建立连接。默认什么也不做（本地/内存实现用不着）。
    ///
    /// **重试前必须先叫它。** 上传失败最常见的原因就是连接本身坏了——控制连接被
    /// 服务器按空闲超时踢掉、NAT 会话过期、网络切换。拿同一条坏连接重试三次，
    /// 三次都会失败：那样的重试「看着有、其实没有」，还白等两轮退避。
    ///
    /// 失败不算致命：接着用旧连接再试一次上传，让上传自己的错误去决定成败。
    fn reconnect(&mut self) -> Result<()> {
        Ok(())
    }
}

/// 算出这次同步会传什么。只读远端，不写一个字节。
///
/// `sync` 与 `plan` 共用它：两处各算一遍，迟早出现「预览说传 3 个、实际传 30 个」。
///
/// 第三个返回值是比对阶段那一次进度回调的答复：比对本身也可能要查上千次远端状态，
/// 用户在这期间点「停止」，两边（真发布 / 干跑）各自决定怎么收场。
fn compute(
    fs: &mut dyn RemoteFs,
    dist_dir: &Path,
    overwrite: FtpOverwrite,
    progress: ProgressFn<'_>,
) -> Result<(Vec<manifest::LocalEntry>, manifest::SyncPlan, Flow)> {
    ensure_output_ready(dist_dir)?;

    let local = manifest::scan(dist_dir)?;
    let flow = progress(Progress {
        message: format!("比对远端状态（{} 个文件）", local.len()),
        current: 0,
        total: local.len(),
    });

    // 「总是上传」不必逐个 stat：省下 N 次往返，这正是选它的人想要的确定性。
    let mut remote = Vec::new();
    if overwrite != FtpOverwrite::Always && flow != Flow::Stop {
        for entry in &local {
            if let Some(found) = fs.stat(&entry.path)? {
                remote.push(found);
            }
        }
    }

    let plan = manifest::plan_sync(&local, &remote, overwrite);
    Ok((local, plan, flow))
}

/// 干跑：会传哪些文件、跳过几个、总共多少字节。
///
/// 「总是上传」之外的四种规则必须连远端才能判断（材料只有 `SIZE` 与 `MDTM`），
/// 所以这一步会连服务器——但只读。这件事写进 `warnings` 里告诉用户，
/// 而不是让「干跑」听起来像完全离线。
pub fn plan(
    fs: &mut dyn RemoteFs,
    dist_dir: &Path,
    target: &str,
    overwrite: FtpOverwrite,
    progress: ProgressFn<'_>,
) -> Result<DeployPlan> {
    let (local, plan, flow) = compute(fs, dist_dir, overwrite, progress)?;
    if flow.is_stop() {
        // 干跑停下来只能是错误：一份少了一半的清单在界面上和完整清单长得一样。
        return Err(Error::Cancelled(
            "发布预览已按要求停止，因此不给出清单：残缺的清单会被当成完整的预览来读".to_string(),
        ));
    }
    let mut warnings = vec![if overwrite == FtpOverwrite::Always {
        "覆盖规则是「总是覆盖」，因此没有查询远端状态：远端已有的同名文件都会被覆盖".to_string()
    } else {
        "已逐个查询远端文件的大小与时间（只读），没有写入任何内容".to_string()
    }];
    // 远端多余的文件既不删、也**没有去查**：`compute` 是逐个 stat 本地文件问出来的，
    // 从不列远端目录（`RemoteFs` 没有列目录这个能力）。所以这里只能说到这个程度——
    // 说成「远端多余的文件不会被删除」会让人以为我们知道有哪些，那是我们不知道的事。
    warnings.push(format!(
        "只逐个查了本地这 {} 个文件对应的远端状态；远端另外还有什么文件、要不要清，这里查不到也不会动",
        local.len()
    ));

    Ok(DeployPlan {
        target: target.to_string(),
        bytes: manifest::total_bytes(&local, &plan.upload),
        upload: plan.upload,
        skipped: plan.skipped,
        warnings,
    })
}

/// 执行一次差异同步。
///
/// `overwrite` 决定远端已存在同名文件时怎么办（见 `FtpOverwrite`）：判定材料只有大小
/// 与 `MDTM`，哪一项可信取决于用户那台服务器，所以这是配置项而不是我们的推断。
///
/// 远端多余文件不会被删除——静态站点常混有手工上传的资源，
/// 静默删除的代价远高于留下少量陈旧文件。
///
/// `progress` 返回 [`Flow::Stop`] 时在**文件边界**停下：当前这个文件要么整个传完、
/// 要么根本没开始传，绝不留下半个文件。
pub fn sync(
    fs: &mut dyn RemoteFs,
    dist_dir: &Path,
    target: &str,
    overwrite: FtpOverwrite,
    progress: ProgressFn<'_>,
) -> Result<DeployReport> {
    let started = Instant::now();
    let (local, plan, flow) = compute(fs, dist_dir, overwrite, progress)?;
    let total = plan.upload.len();
    if flow.is_stop() {
        // 比对阶段就被叫停：一个字节都还没写，连目录都不必建。
        return Ok(cancelled(target, Vec::new(), plan.skipped, total, started));
    }
    for dir in manifest::required_directories(&plan.upload) {
        fs.mkdir(&dir)?;
    }

    let mut uploaded = Vec::with_capacity(total);
    for (i, path) in plan.upload.iter().enumerate() {
        let entry = local
            .iter()
            .find(|l| &l.path == path)
            .expect("上传列表来自本地清单");
        // 停止判断放在上传之前：此刻前面每个文件都已完整传完，是最干净的边界。
        if progress(Progress {
            message: format!("上传 {path}"),
            current: i + 1,
            total,
        })
        .is_stop()
        {
            return Ok(cancelled(target, uploaded, plan.skipped, total, started));
        }
        // 上传严格顺序进行，所以「已成功几个」就是 i：走到这里说明前面每一个都成了。
        match upload_with_retry(fs, &entry.absolute, path, i + 1, total, progress) {
            Ok(Flow::Continue) => uploaded.push(path.clone()),
            // 重试的间隙里被叫停：这个文件没传上去，边界依然干净。
            Ok(Flow::Stop) => return Ok(cancelled(target, uploaded, plan.skipped, total, started)),
            Err(err) => return Err(interrupted(path, i, total, &err)),
        }
    }

    Ok(DeployReport {
        target: target.to_string(),
        uploaded,
        deleted: Vec::new(),
        skipped: plan.skipped,
        duration_ms: started.elapsed().as_millis() as u64,
        commit: None,
        warnings: Vec::new(),
        cancelled: false,
    })
}

/// 被叫停之后的报告：说清「传到哪儿了、现在安不安全」。
///
/// 与 [`interrupted`] 是同一件事的两种成因（用户叫停 / 上传失败），话也必须一样清楚：
/// 逐文件覆盖没有原子性，停下来的那一刻线上就是「一半新一半旧」。
fn cancelled(
    target: &str,
    uploaded: Vec<String>,
    skipped: usize,
    total: usize,
    started: Instant,
) -> DeployReport {
    let done = uploaded.len();
    DeployReport {
        target: target.to_string(),
        uploaded,
        deleted: Vec::new(),
        skipped,
        duration_ms: started.elapsed().as_millis() as u64,
        commit: None,
        warnings: vec![format!(
            "已按要求停止：这一批 {total} 个文件里传完了 {done} 个。\
             逐文件覆盖不是原子的，线上此刻是「一半新一半旧」；\
             想继续就再发一次，已经传上去的那些会被跳过。"
        )],
        cancelled: true,
    }
}

/// 上传一个文件，失败时重试几次。
///
/// 每次重试都往进度里报一句：一次发布可能卡在某个大文件上重试好几秒，
/// 界面上不说明的话看起来就是「卡住了」。
///
/// 返回 [`Flow::Stop`] 表示用户在重试的间隙里叫停，这个文件**没有**传上去——
/// 这恰恰是最想停的时刻：卡在某个文件上反复重试，等下去也未必有结果。
fn upload_with_retry(
    fs: &mut dyn RemoteFs,
    local: &Path,
    remote: &str,
    current: usize,
    total: usize,
    progress: ProgressFn<'_>,
) -> Result<Flow> {
    let mut attempt = 1;
    loop {
        match fs.upload(local, remote) {
            Ok(()) => return Ok(Flow::Continue),
            Err(err) if attempt >= UPLOAD_ATTEMPTS => return Err(err),
            Err(err) => {
                tracing::warn!("上传 {remote} 第 {attempt} 次失败，准备重连后重试: {err}");
                attempt += 1;
                let flow = progress(Progress {
                    message: format!("上传 {remote} 失败，重连后第 {attempt} 次尝试"),
                    current,
                    total,
                });
                if flow.is_stop() {
                    return Ok(Flow::Stop);
                }
                std::thread::sleep(RETRY_PAUSE);
                // 先重连再重试：失败最常见的原因就是这条连接已经坏了。
                // 重连本身失败也接着试上传——让上传的错误去决定成败，
                // 而不是在这里把「重连失败」当成最终原因报出去（那句话对用户没用）。
                if let Err(reconnect_err) = fs.reconnect() {
                    tracing::warn!("重连失败，继续用旧连接重试: {reconnect_err}");
                }
            }
        }
    }
}

/// 重试用尽之后的错误：必须说清「传到哪儿了」。
///
/// 逐文件覆盖没有原子性，中断时线上一定是「一半新一半旧」。只报「上传 X 失败」
/// 的话，用户既不知道已经推上去多少，也不知道重来一次安不安全——
/// 而答案是安全的：差异同步会跳过已经传上去的那些。
fn interrupted(path: &str, done: usize, total: usize, err: &Error) -> Error {
    Error::Other(format!(
        "发布中断：上传 {path} 失败（这一批 {total} 个里已成功 {done} 个）。\
         逐文件覆盖不是原子的，线上此刻是「一半新一半旧」；\
         处理好原因后再发一次即可，已经传上去的那些会被跳过。原因：{err}"
    ))
}

#[cfg(feature = "ftp")]
pub use plain::FtpDeployer;

#[cfg(feature = "ftp")]
mod plain {
    use std::path::Path;

    use suppaftp::FtpStream;

    use super::{plan, sync, RemoteFs};
    use crate::manifest::RemoteEntry;
    use crate::{Credentials, DeployPlan, DeployReport, Deployer, Error, ProgressFn, Result};
    use staticsmith_core::config::FtpOverwrite;

    /// 明文 FTP 发布通道。
    pub struct FtpDeployer {
        pub host: String,
        pub port: u16,
        pub remote_path: String,
        pub credentials: Credentials,
        pub overwrite: FtpOverwrite,
    }

    impl FtpDeployer {
        pub fn new(host: impl Into<String>, port: u16, remote_path: impl Into<String>) -> Self {
            Self {
                host: host.into(),
                port,
                remote_path: remote_path.into(),
                credentials: Credentials::None,
                overwrite: FtpOverwrite::default(),
            }
        }

        pub fn with_credentials(mut self, credentials: Credentials) -> Self {
            self.credentials = credentials;
            self
        }

        pub fn with_overwrite(mut self, overwrite: FtpOverwrite) -> Self {
            self.overwrite = overwrite;
            self
        }

        fn connect(&self) -> Result<FtpStream> {
            let address = format!("{}:{}", self.host, self.port);
            let mut stream =
                FtpStream::connect(&address).map_err(|e| Error::Ftp(format!("{address}: {e}")))?;
            match &self.credentials {
                Credentials::UserPassword { username, password } => {
                    stream
                        .login(username, password)
                        .map_err(|e| Error::Ftp(e.to_string()))?;
                }
                Credentials::None => {
                    stream
                        .login("anonymous", "anonymous@")
                        .map_err(|e| Error::Ftp(e.to_string()))?;
                }
                Credentials::SshKey { .. } => {
                    return Err(Error::Credentials(
                        "明文 FTP 不支持 SSH 私钥，请改用 SFTP".to_string(),
                    ));
                }
            }
            stream
                .cwd(&self.remote_path)
                .map_err(|e| Error::Ftp(format!("切换到 {}: {e}", self.remote_path)))?;
            Ok(stream)
        }
    }

    struct FtpFs<'a> {
        stream: FtpStream,
        /// 重连要用的那份连接参数。借用而不是复制：参数的唯一来源是 deployer 自己。
        deployer: &'a FtpDeployer,
    }

    impl RemoteFs for FtpFs<'_> {
        fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>> {
            let Ok(size) = self.stream.size(path) else {
                return Ok(None); // 550：文件不存在
            };
            let modified_epoch = self
                .stream
                .mdtm(path)
                .ok()
                .map(|dt| dt.and_utc().timestamp());
            Ok(Some(RemoteEntry {
                path: path.to_string(),
                size: size as u64,
                modified_epoch,
            }))
        }

        fn mkdir(&mut self, path: &str) -> Result<()> {
            // 目录已存在时服务器返回 550，忽略即可。
            let _ = self.stream.mkdir(path);
            Ok(())
        }

        fn upload(&mut self, local: &Path, remote: &str) -> Result<()> {
            let mut file = std::fs::File::open(local).map_err(|e| Error::io(local, e))?;
            self.stream
                .put_file(remote, &mut file)
                .map(|_| ())
                .map_err(|e| Error::Ftp(format!("上传 {remote}: {e}")))
        }

        fn reconnect(&mut self) -> Result<()> {
            // 旧连接可能只是半死（控制连接还在、数据连接超时），先礼貌关一下，
            // 失败无所谓——它已经不可信了。
            let _ = self.stream.quit();
            self.stream = self.deployer.connect()?;
            Ok(())
        }
    }

    impl Deployer for FtpDeployer {
        fn deploy(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployReport> {
            let mut fs = FtpFs {
                stream: self.connect()?,
                deployer: self,
            };
            let target = format!("ftp://{}{}", self.host, self.remote_path);
            let report = sync(&mut fs, dist_dir, &target, self.overwrite, progress);
            let _ = fs.stream.quit();
            report
        }

        fn plan(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployPlan> {
            let mut fs = FtpFs {
                stream: self.connect()?,
                deployer: self,
            };
            let target = format!("ftp://{}{}", self.host, self.remote_path);
            let planned = plan(&mut fs, dist_dir, &target, self.overwrite, progress);
            let _ = fs.stream.quit();
            planned
        }

        fn check(&self) -> Result<()> {
            let mut stream = self.connect()?;
            let _ = stream.quit();
            Ok(())
        }
    }
}

#[cfg(feature = "sftp")]
pub use secure::SftpDeployer;

#[cfg(feature = "sftp")]
mod secure {
    use std::io::Write;
    use std::net::TcpStream;
    use std::path::{Path, PathBuf};

    use ssh2::Session;

    use super::{plan, sync, RemoteFs};
    use crate::manifest::RemoteEntry;
    use crate::{Credentials, DeployPlan, DeployReport, Deployer, Error, ProgressFn, Result};
    use staticsmith_core::config::FtpOverwrite;

    /// SFTP（SSH）发布通道。
    pub struct SftpDeployer {
        pub host: String,
        pub port: u16,
        pub remote_path: String,
        pub credentials: Credentials,
        pub overwrite: FtpOverwrite,
    }

    impl SftpDeployer {
        pub fn new(host: impl Into<String>, port: u16, remote_path: impl Into<String>) -> Self {
            Self {
                host: host.into(),
                port,
                remote_path: remote_path.into(),
                credentials: Credentials::None,
                overwrite: FtpOverwrite::default(),
            }
        }

        pub fn with_credentials(mut self, credentials: Credentials) -> Self {
            self.credentials = credentials;
            self
        }

        pub fn with_overwrite(mut self, overwrite: FtpOverwrite) -> Self {
            self.overwrite = overwrite;
            self
        }

        fn session(&self) -> Result<Session> {
            let tcp = TcpStream::connect((self.host.as_str(), self.port))
                .map_err(|e| Error::io(&self.host, e))?;
            let mut session = Session::new()?;
            session.set_tcp_stream(tcp);
            session.handshake()?;
            match &self.credentials {
                Credentials::UserPassword { username, password } => {
                    session.userauth_password(username, password)?
                }
                Credentials::SshKey {
                    username,
                    private_key,
                    passphrase,
                } => session.userauth_pubkey_file(
                    username,
                    None,
                    private_key,
                    passphrase.as_deref(),
                )?,
                Credentials::None => {
                    return Err(Error::Credentials("SFTP 必须提供凭证".to_string()))
                }
            }
            if !session.authenticated() {
                return Err(Error::Credentials("SFTP 认证失败".to_string()));
            }
            Ok(session)
        }
    }

    struct SftpFs<'a> {
        sftp: ssh2::Sftp,
        base: PathBuf,
        /// 重连要用的那份连接参数。借用而不是复制：参数的唯一来源是 deployer 自己。
        deployer: &'a SftpDeployer,
    }

    impl SftpFs<'_> {
        fn absolute(&self, path: &str) -> PathBuf {
            self.base.join(path)
        }
    }

    impl RemoteFs for SftpFs<'_> {
        fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>> {
            match self.sftp.stat(&self.absolute(path)) {
                Ok(stat) => Ok(Some(RemoteEntry {
                    path: path.to_string(),
                    size: stat.size.unwrap_or(0),
                    modified_epoch: stat.mtime.map(|m| m as i64),
                })),
                Err(_) => Ok(None),
            }
        }

        fn mkdir(&mut self, path: &str) -> Result<()> {
            let _ = self.sftp.mkdir(&self.absolute(path), 0o755);
            Ok(())
        }

        fn upload(&mut self, local: &Path, remote: &str) -> Result<()> {
            let data = std::fs::read(local).map_err(|e| Error::io(local, e))?;
            let mut file = self.sftp.create(&self.absolute(remote))?;
            file.write_all(&data)
                .map_err(|e| Error::io(self.absolute(remote), e))
        }

        fn reconnect(&mut self) -> Result<()> {
            // ssh2 的 Sftp 句柄绑在 Session 上，重开一条会话即可；旧的随句柄一起丢掉。
            self.sftp = self.deployer.session()?.sftp()?;
            Ok(())
        }
    }

    impl Deployer for SftpDeployer {
        fn deploy(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployReport> {
            let session = self.session()?;
            let mut fs = SftpFs {
                sftp: session.sftp()?,
                base: PathBuf::from(&self.remote_path),
                deployer: self,
            };
            let target = format!("sftp://{}{}", self.host, self.remote_path);
            sync(&mut fs, dist_dir, &target, self.overwrite, progress)
        }

        fn plan(&self, dist_dir: &Path, progress: ProgressFn<'_>) -> Result<DeployPlan> {
            let session = self.session()?;
            let mut fs = SftpFs {
                sftp: session.sftp()?,
                base: PathBuf::from(&self.remote_path),
                deployer: self,
            };
            let target = format!("sftp://{}{}", self.host, self.remote_path);
            plan(&mut fs, dist_dir, &target, self.overwrite, progress)
        }

        fn check(&self) -> Result<()> {
            let session = self.session()?;
            session.sftp()?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::Error;

    /// 内存假远端，用于验证同步算法。
    #[derive(Default)]
    struct FakeRemote {
        files: BTreeMap<String, (u64, Option<i64>)>,
        dirs: Vec<String>,
        uploads: Vec<String>,
        stats: Vec<String>,
        /// 这个路径的前 N 次上传要失败，模拟网络抖动。`usize::MAX` 表示一直失败。
        flaky: BTreeMap<String, usize>,
        /// 每个路径实际被尝试上传了几次。
        attempts: BTreeMap<String, usize>,
        /// 连接已经坏了：所有上传都失败，直到重连。
        dead: bool,
        /// 重连被调用了几次。
        reconnects: usize,
        /// 重连本身也失败（模拟服务器整个不见了）。
        reconnect_fails: bool,
    }

    impl RemoteFs for FakeRemote {
        fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>> {
            self.stats.push(path.to_string());
            Ok(self.files.get(path).map(|(size, modified)| RemoteEntry {
                path: path.to_string(),
                size: *size,
                modified_epoch: *modified,
            }))
        }

        fn mkdir(&mut self, path: &str) -> Result<()> {
            self.dirs.push(path.to_string());
            Ok(())
        }

        fn upload(&mut self, local: &Path, remote: &str) -> Result<()> {
            *self.attempts.entry(remote.to_string()).or_default() += 1;
            if self.dead {
                return Err(Error::Ftp(format!("连接已断开: {remote}")));
            }
            if let Some(left) = self.flaky.get_mut(remote) {
                if *left > 0 {
                    *left = left.saturating_sub(1);
                    return Err(Error::Ftp(format!("连接被重置: {remote}")));
                }
            }
            let size = std::fs::metadata(local).unwrap().len();
            self.files.insert(remote.to_string(), (size, None));
            self.uploads.push(remote.to_string());
            Ok(())
        }

        fn reconnect(&mut self) -> Result<()> {
            self.reconnects += 1;
            if self.reconnect_fails {
                return Err(Error::Ftp("重连失败：连不上".to_string()));
            }
            self.dead = false;
            Ok(())
        }
    }

    fn dist() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("posts/page/2")).unwrap();
        std::fs::write(dir.path().join("index.html"), "home").unwrap();
        std::fs::write(dir.path().join("posts/index.html"), "posts").unwrap();
        std::fs::write(dir.path().join("posts/page/2/index.html"), "page2").unwrap();
        dir
    }

    fn sync_default(
        remote: &mut FakeRemote,
        dist_dir: &Path,
        target: &str,
    ) -> Result<DeployReport> {
        sync(
            remote,
            dist_dir,
            target,
            FtpOverwrite::default(),
            &mut |_| Flow::Continue,
        )
    }

    #[test]
    fn plan_tells_what_would_be_uploaded_without_writing_anything() {
        let dir = dist();
        let mut remote = FakeRemote::default();

        let planned = plan(
            &mut remote,
            dir.path(),
            "ftp://example.test/www",
            FtpOverwrite::default(),
            &mut |_| Flow::Continue,
        )
        .unwrap();

        assert_eq!(planned.target, "ftp://example.test/www");
        assert_eq!(planned.upload.len(), 3);
        assert_eq!(planned.skipped, 0);
        // "home" + "posts" + "page2"
        assert_eq!(planned.bytes, 14);
        // 一个字节都不该写：既不建目录也不上传
        assert!(remote.uploads.is_empty(), "{:?}", remote.uploads);
        assert!(remote.dirs.is_empty(), "{:?}", remote.dirs);
        // 但确实查过远端状态，而且这件事要写进 warnings，不能让「干跑」听起来完全离线
        assert_eq!(remote.stats.len(), 3);
        assert!(
            planned.warnings.iter().any(|w| w.contains("只读")),
            "{:?}",
            planned.warnings
        );
        assert!(
            planned
                .warnings
                .iter()
                .any(|w| w.contains("查不到也不会动")),
            "{:?}",
            planned.warnings
        );
    }

    /// 干跑要说清「查了什么、没查什么」。
    ///
    /// 原先那句是「远端多余的文件不会被删除」——听起来像我们知道有哪些多余文件，
    /// 而这一层根本查不到：`compute` 只逐个 stat 本地文件对应的远端状态，从不列远端目录。
    /// 说得比知道的多，是这套东西里最不该有的毛病。
    #[test]
    fn plan_says_what_it_did_not_look_at() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        for path in ["old/index.html", "gone.html"] {
            remote.files.insert(path.into(), (10, None));
        }

        let planned = plan(
            &mut remote,
            dir.path(),
            "ftp://example.test/www",
            FtpOverwrite::default(),
            &mut |_| Flow::Continue,
        )
        .unwrap();

        let notice = planned
            .warnings
            .iter()
            .find(|w| w.contains("查不到"))
            .unwrap_or_else(|| panic!("{:?}", planned.warnings));
        // 查了几个说得出来（本地 3 个），没查的部分不许假装知道
        assert!(notice.contains("3 个"), "{notice}");
        assert!(!notice.contains("gone.html"), "{notice}");

        // 干跑一个字节都不写，远端那几个旧文件也还在
        assert!(remote.uploads.is_empty());
        assert!(remote.files.contains_key("gone.html"));
    }

    #[test]
    fn plan_and_sync_agree_on_what_changes() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        sync_default(&mut remote, dir.path(), "t").unwrap();

        // 改一个文件，另两个不动
        std::fs::write(dir.path().join("index.html"), "home v2").unwrap();

        let planned = plan(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::default(),
            &mut |_| Flow::Continue,
        )
        .unwrap();
        assert_eq!(planned.upload, vec!["index.html".to_string()]);
        assert_eq!(planned.skipped, 2);

        let report = sync_default(&mut remote, dir.path(), "t").unwrap();
        assert_eq!(report.uploaded, planned.upload, "预览说传哪个就得传哪个");
        assert_eq!(report.skipped, planned.skipped);
    }

    #[test]
    fn plan_says_it_skipped_the_remote_lookup_when_always_overwriting() {
        let dir = dist();
        let mut remote = FakeRemote::default();

        let planned = plan(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::Always,
            &mut |_| Flow::Continue,
        )
        .unwrap();

        assert_eq!(planned.upload.len(), 3);
        assert!(remote.stats.is_empty(), "「总是覆盖」不该查远端");
        assert!(
            planned.warnings.iter().any(|w| w.contains("总是覆盖")),
            "{:?}",
            planned.warnings
        );
    }

    #[test]
    fn first_sync_uploads_everything_and_creates_directories() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        let mut events = Vec::new();

        let report = sync(
            &mut remote,
            dir.path(),
            "ftp://example.com",
            FtpOverwrite::default(),
            &mut |p| {
                events.push(p.message);
                Flow::Continue
            },
        )
        .unwrap();

        assert_eq!(report.uploaded.len(), 3);
        assert_eq!(report.skipped, 0);
        assert_eq!(remote.dirs, vec!["posts", "posts/page", "posts/page/2"]);
        assert_eq!(events.len(), 4, "1 条比对 + 3 条上传");
    }

    #[test]
    fn second_sync_skips_unchanged_files() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        sync_default(&mut remote, dir.path(), "t").unwrap();
        remote.uploads.clear();

        let report = sync_default(&mut remote, dir.path(), "t").unwrap();
        assert!(report.uploaded.is_empty());
        assert_eq!(report.skipped, 3);
        assert!(remote.uploads.is_empty());
    }

    #[test]
    fn changed_file_is_reuploaded() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        sync_default(&mut remote, dir.path(), "t").unwrap();

        std::fs::write(dir.path().join("index.html"), "home page updated").unwrap();
        let report = sync_default(&mut remote, dir.path(), "t").unwrap();

        assert_eq!(report.uploaded, vec!["index.html"]);
        assert_eq!(report.skipped, 2);
    }

    /// 「总是上传」既要全传，也不该白跑 N 次 `stat`——省下的正是往返次数。
    #[test]
    fn always_rule_uploads_everything_without_asking_the_remote() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        sync_default(&mut remote, dir.path(), "t").unwrap();
        remote.uploads.clear();
        remote.stats.clear();

        let report = sync(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::Always,
            &mut |_| Flow::Continue,
        )
        .unwrap();

        assert_eq!(report.uploaded.len(), 3);
        assert_eq!(report.skipped, 0);
        assert!(remote.stats.is_empty(), "不比对就不该查远端状态");
    }

    /// 网络抖一下不该让整次发布作废。
    ///
    /// FTP 上传是逐文件的，一次连接重置以前会直接冒泡成失败——而线上此刻已经是
    /// 「一半新一半旧」。同一个文件重传几次的代价远小于让用户自己判断该不该重来。
    #[test]
    fn a_flaky_upload_is_retried() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        // 前两次失败，第三次成功。
        remote.flaky.insert("index.html".to_string(), 2);

        let report = sync_default(&mut remote, dir.path(), "t").unwrap();

        assert_eq!(report.uploaded.len(), 3, "抖动的那个也要传上去");
        assert_eq!(remote.attempts.get("index.html"), Some(&3));
        assert_eq!(
            remote.attempts.get("posts/index.html"),
            Some(&1),
            "别的文件不该被牵连着重传"
        );
    }

    /// 重试之前必须先重连，否则这套重试在最需要它的场景下必然无效。
    ///
    /// 上传失败最常见的原因就是连接本身坏了（控制连接被空闲超时踢掉、NAT 会话过期、
    /// 网络切换）。拿同一条坏连接重试三次，三次都会失败——那样的重试「看着有、其实没有」，
    /// 还白等两轮退避。
    #[test]
    fn a_broken_connection_is_rebuilt_before_retrying() {
        let dir = dist();
        let mut remote = FakeRemote {
            dead: true,
            ..FakeRemote::default()
        };

        let report = sync_default(&mut remote, dir.path(), "t").unwrap();

        assert_eq!(remote.reconnects, 1, "只该重连一次：一次就把连接修好了");
        assert_eq!(report.uploaded.len(), 3, "重连之后三个都要传上去");
        // 第一个文件失败一次、重连后成功，所以它被尝试了两次；后面的都是一次过
        assert_eq!(remote.attempts.get("index.html"), Some(&2));
        assert_eq!(remote.attempts.get("posts/index.html"), Some(&1));
    }

    /// 重连失败不许盖住真正的原因。
    ///
    /// 用户要的答案是「上传为什么失败、传到哪儿了」，而不是「重连也失败了」——
    /// 后者只是我们自救过程中的一个中间状态。
    #[test]
    fn a_failed_reconnect_does_not_replace_the_upload_error() {
        let dir = dist();
        let mut remote = FakeRemote {
            dead: true,
            reconnect_fails: true,
            ..FakeRemote::default()
        };

        let err = sync_default(&mut remote, dir.path(), "t")
            .unwrap_err()
            .to_string();

        assert!(err.contains("连接已断开"), "{err}");
        assert!(!err.contains("重连失败"), "{err}");
        // 仍然按 UPLOAD_ATTEMPTS 试满，并且每次都试着重连
        assert_eq!(remote.attempts.get("index.html"), Some(&UPLOAD_ATTEMPTS));
        assert_eq!(remote.reconnects, UPLOAD_ATTEMPTS - 1);
    }

    /// 重试用尽之后，错误要说清「传到哪儿了」。
    ///
    /// 逐文件覆盖没有原子性，失败时线上一定处于「一半新一半旧」。只说
    /// 「上传 X 失败」的话，用户不知道已经传了多少、也不知道重来一次是否安全。
    #[test]
    fn a_hopeless_upload_reports_how_far_it_got() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        remote
            .flaky
            .insert("posts/index.html".to_string(), usize::MAX);

        let err = sync_default(&mut remote, dir.path(), "t").unwrap_err();
        let message = err.to_string();

        assert!(message.contains("posts/index.html"), "{message}");
        assert!(message.contains("已成功"), "{message}");
        assert!(message.contains("再发一次"), "{message}");
        // 三个文件里 index.html 排在前面，它应该已经传上去了。
        assert!(remote.uploads.contains(&"index.html".to_string()));
    }

    #[test]
    fn extra_remote_files_are_left_untouched() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        remote
            .files
            .insert("legacy/uploads/a.pdf".into(), (10, None));

        let report = sync_default(&mut remote, dir.path(), "t").unwrap();
        assert!(report.deleted.is_empty());
        assert!(remote.files.contains_key("legacy/uploads/a.pdf"));
    }

    /// 在第 n 次进度回调上说「停」的回调；顺带记下所有消息。
    fn stop_at(nth: usize, events: &mut Vec<String>) -> impl FnMut(Progress) -> Flow + '_ {
        let mut seen = 0;
        move |p: Progress| {
            seen += 1;
            events.push(p.message);
            if seen >= nth {
                Flow::Stop
            } else {
                Flow::Continue
            }
        }
    }

    /// 发布能中途停下来，而且停下来之后要说清「传到哪儿了」。
    ///
    /// 这是三个长操作里唯一天然有停止点的：文件之间的边界本来就不是原子的，
    /// 中断处理早就写好了。停止不是错误——「已经传上去几个」是此刻唯一重要的信息，
    /// 做成 `Err` 就会被一句错误吞掉。
    #[test]
    fn stopping_between_files_reports_how_far_it_got() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        let mut events = Vec::new();

        // 回调顺序是「1 条比对 + 每个文件 1 条」，第 3 条时第一个文件已经传完。
        let report = sync(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::default(),
            &mut stop_at(3, &mut events),
        )
        .unwrap();

        assert!(report.cancelled, "停止是合法结果，不该变成错误");
        assert_eq!(report.uploaded.len(), 1, "{:?}", report.uploaded);
        assert_eq!(
            report.uploaded, remote.uploads,
            "报告里说传了哪些，远端就得只有哪些"
        );
        let warning = report.warnings.first().unwrap();
        assert!(warning.contains("传完了 1 个"), "{warning}");
        assert!(warning.contains("一半新一半旧"), "{warning}");
        assert!(warning.contains("再发一次"), "{warning}");
    }

    /// 比对阶段就叫停：一个字节都不该写，连目录都不该建。
    ///
    /// 比对本身可能要查上千次远端状态，是最容易让人想按停止的地方。
    #[test]
    fn stopping_during_the_compare_phase_writes_nothing() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        let mut events = Vec::new();

        let report = sync(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::default(),
            &mut stop_at(1, &mut events),
        )
        .unwrap();

        assert!(report.cancelled);
        assert!(report.uploaded.is_empty());
        assert!(remote.uploads.is_empty());
        assert!(remote.dirs.is_empty(), "连目录都不必建: {:?}", remote.dirs);
        assert_eq!(events.len(), 1, "停了就不该再报进度: {events:?}");
    }

    /// 卡在某个文件上反复重试，正是最想停的时刻；停下来时这个文件不能只传一半。
    #[test]
    fn stopping_while_retrying_leaves_the_file_alone() {
        let dir = dist();
        let mut remote = FakeRemote {
            dead: true,
            ..FakeRemote::default()
        };
        let mut events = Vec::new();

        // 1 条比对 + 1 条「上传 index.html」+ 1 条「失败，重连后第 2 次尝试」
        let report = sync(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::default(),
            &mut stop_at(3, &mut events),
        )
        .unwrap();

        assert!(report.cancelled);
        assert!(report.uploaded.is_empty(), "{:?}", report.uploaded);
        assert!(remote.files.is_empty(), "不许留下半个文件");
        assert_eq!(remote.reconnects, 0, "已经要停了就别再重连");
        assert!(events.last().unwrap().contains("第 2 次尝试"), "{events:?}");
    }

    /// 干跑停下来只能是错误。
    ///
    /// 一份少了一半的清单，在界面上和完整清单长得一模一样——用户会照着它做决定：
    /// 「只要传 3 个」。真发布停下来是合法结果，干跑停下来不是。
    #[test]
    fn a_stopped_plan_refuses_to_hand_back_half_a_list() {
        let dir = dist();
        let mut remote = FakeRemote::default();

        let err = plan(
            &mut remote,
            dir.path(),
            "t",
            FtpOverwrite::default(),
            &mut |_| Flow::Stop,
        )
        .unwrap_err();

        assert!(matches!(err, Error::Cancelled(_)), "{err:?}");
        assert!(err.to_string().contains("残缺"), "{err}");
        // 叫停之后连远端状态都不必查了
        assert!(remote.stats.is_empty(), "{:?}", remote.stats);
    }
}
