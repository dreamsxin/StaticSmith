//! FTP / SFTP 发布：`rsync` 风格差异同步。
//!
//! 同步算法与协议解耦：[`RemoteFs`] 抽象出「查状态 / 建目录 / 上传」三个原语，
//! [`sync`] 负责比对与推进进度，因此可以用内存假实现完整测试同步逻辑。

use std::path::Path;
use std::time::Instant;

use crate::manifest::{self, RemoteEntry};
use crate::{ensure_output_ready, DeployReport, Progress, Result};

/// 远端文件系统原语。
pub trait RemoteFs {
    /// 查询远端文件状态；不存在返回 `Ok(None)`。
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>>;
    /// 创建目录，已存在时不应报错。
    fn mkdir(&mut self, path: &str) -> Result<()>;
    /// 上传单个文件。
    fn upload(&mut self, local: &Path, remote: &str) -> Result<()>;
}

/// 执行一次差异同步。
///
/// 远端多余文件不会被删除——静态站点常混有手工上传的资源，
/// 静默删除的代价远高于留下少量陈旧文件。
pub fn sync(
    fs: &mut dyn RemoteFs,
    dist_dir: &Path,
    target: &str,
    progress: &mut dyn FnMut(Progress),
) -> Result<DeployReport> {
    let started = Instant::now();
    ensure_output_ready(dist_dir)?;

    let local = manifest::scan(dist_dir)?;
    progress(Progress {
        message: format!("比对远端状态（{} 个文件）", local.len()),
        current: 0,
        total: local.len(),
    });

    let mut remote = Vec::new();
    for entry in &local {
        if let Some(found) = fs.stat(&entry.path)? {
            remote.push(found);
        }
    }

    let plan = manifest::plan_sync(&local, &remote, false);

    for dir in manifest::required_directories(&plan.upload) {
        fs.mkdir(&dir)?;
    }

    let total = plan.upload.len();
    for (i, path) in plan.upload.iter().enumerate() {
        let entry = local
            .iter()
            .find(|l| &l.path == path)
            .expect("上传列表来自本地清单");
        progress(Progress {
            message: format!("上传 {path}"),
            current: i + 1,
            total,
        });
        fs.upload(&entry.absolute, path)?;
    }

    Ok(DeployReport {
        target: target.to_string(),
        uploaded: plan.upload,
        deleted: Vec::new(),
        skipped: plan.skipped,
        duration_ms: started.elapsed().as_millis() as u64,
        commit: None,
        warnings: Vec::new(),
    })
}

#[cfg(feature = "ftp")]
pub use plain::FtpDeployer;

#[cfg(feature = "ftp")]
mod plain {
    use std::path::Path;

    use suppaftp::FtpStream;

    use super::{sync, RemoteFs};
    use crate::manifest::RemoteEntry;
    use crate::{Credentials, DeployReport, Deployer, Error, Progress, Result};

    /// 明文 FTP 发布通道。
    pub struct FtpDeployer {
        pub host: String,
        pub port: u16,
        pub remote_path: String,
        pub credentials: Credentials,
    }

    impl FtpDeployer {
        pub fn new(host: impl Into<String>, port: u16, remote_path: impl Into<String>) -> Self {
            Self {
                host: host.into(),
                port,
                remote_path: remote_path.into(),
                credentials: Credentials::None,
            }
        }

        pub fn with_credentials(mut self, credentials: Credentials) -> Self {
            self.credentials = credentials;
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

    struct FtpFs {
        stream: FtpStream,
    }

    impl RemoteFs for FtpFs {
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
    }

    impl Deployer for FtpDeployer {
        fn deploy(
            &self,
            dist_dir: &Path,
            progress: &mut dyn FnMut(Progress),
        ) -> Result<DeployReport> {
            let mut fs = FtpFs {
                stream: self.connect()?,
            };
            let target = format!("ftp://{}{}", self.host, self.remote_path);
            let report = sync(&mut fs, dist_dir, &target, progress);
            let _ = fs.stream.quit();
            report
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

    use super::{sync, RemoteFs};
    use crate::manifest::RemoteEntry;
    use crate::{Credentials, DeployReport, Deployer, Error, Progress, Result};

    /// SFTP（SSH）发布通道。
    pub struct SftpDeployer {
        pub host: String,
        pub port: u16,
        pub remote_path: String,
        pub credentials: Credentials,
    }

    impl SftpDeployer {
        pub fn new(host: impl Into<String>, port: u16, remote_path: impl Into<String>) -> Self {
            Self {
                host: host.into(),
                port,
                remote_path: remote_path.into(),
                credentials: Credentials::None,
            }
        }

        pub fn with_credentials(mut self, credentials: Credentials) -> Self {
            self.credentials = credentials;
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

    struct SftpFs {
        sftp: ssh2::Sftp,
        base: PathBuf,
    }

    impl SftpFs {
        fn absolute(&self, path: &str) -> PathBuf {
            self.base.join(path)
        }
    }

    impl RemoteFs for SftpFs {
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
    }

    impl Deployer for SftpDeployer {
        fn deploy(
            &self,
            dist_dir: &Path,
            progress: &mut dyn FnMut(Progress),
        ) -> Result<DeployReport> {
            let session = self.session()?;
            let mut fs = SftpFs {
                sftp: session.sftp()?,
                base: PathBuf::from(&self.remote_path),
            };
            let target = format!("sftp://{}{}", self.host, self.remote_path);
            sync(&mut fs, dist_dir, &target, progress)
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

    /// 内存假远端，用于验证同步算法。
    #[derive(Default)]
    struct FakeRemote {
        files: BTreeMap<String, (u64, Option<i64>)>,
        dirs: Vec<String>,
        uploads: Vec<String>,
    }

    impl RemoteFs for FakeRemote {
        fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>> {
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
            let size = std::fs::metadata(local).unwrap().len();
            self.files.insert(remote.to_string(), (size, None));
            self.uploads.push(remote.to_string());
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

    #[test]
    fn first_sync_uploads_everything_and_creates_directories() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        let mut events = Vec::new();

        let report = sync(&mut remote, dir.path(), "ftp://example.com", &mut |p| {
            events.push(p.message)
        })
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
        sync(&mut remote, dir.path(), "t", &mut |_| {}).unwrap();
        remote.uploads.clear();

        let report = sync(&mut remote, dir.path(), "t", &mut |_| {}).unwrap();
        assert!(report.uploaded.is_empty());
        assert_eq!(report.skipped, 3);
        assert!(remote.uploads.is_empty());
    }

    #[test]
    fn changed_file_is_reuploaded() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        sync(&mut remote, dir.path(), "t", &mut |_| {}).unwrap();

        std::fs::write(dir.path().join("index.html"), "home page updated").unwrap();
        let report = sync(&mut remote, dir.path(), "t", &mut |_| {}).unwrap();

        assert_eq!(report.uploaded, vec!["index.html"]);
        assert_eq!(report.skipped, 2);
    }

    #[test]
    fn extra_remote_files_are_left_untouched() {
        let dir = dist();
        let mut remote = FakeRemote::default();
        remote
            .files
            .insert("legacy/uploads/a.pdf".into(), (10, None));

        let report = sync(&mut remote, dir.path(), "t", &mut |_| {}).unwrap();
        assert!(report.deleted.is_empty());
        assert!(remote.files.contains_key("legacy/uploads/a.pdf"));
    }
}
