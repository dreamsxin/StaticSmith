//! 本地预览服务器。
//!
//! 编辑器内嵌的 iframe 用 `srcdoc` 渲染，没有本地文件访问权限，图片、CSS、JS 都取不到。
//! 起一个只监听回环地址的静态文件服务器，预览就与线上完全一致。
//!
//! CLI 的 `staticsmith serve` 与桌面端的「本地预览」共用这一个实现。

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::{Error, Result};

/// 轮询间隔：停止信号最迟在这个时间之后生效。
const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// 版本号查询地址。带前缀是为了不和站点里的真实路径撞车。
const VERSION_PATH: &str = "/__staticsmith/version";

/// 注入进 HTML 的自动刷新脚本。
///
/// 只在预览服务器的响应里注入，产物文件本身不会被改写——发布出去的站点里
/// 不该有这段代码。
///
/// 刷新前把滚动位置记进 `sessionStorage`：写文章时多半正看着页面中段，
/// 每次重新生成都跳回顶部比不刷新更烦。
const LIVE_RELOAD: &str = r#"<script data-staticsmith="live-reload">
(function () {
  var KEY = 'staticsmith.scroll:' + location.pathname;
  var saved = sessionStorage.getItem(KEY);
  if (saved !== null) {
    sessionStorage.removeItem(KEY);
    window.addEventListener('load', function () { window.scrollTo(0, parseInt(saved, 10) || 0); });
  }
  var known = null;
  function poll() {
    fetch('__VERSION_PATH__', { cache: 'no-store' })
      .then(function (r) { return r.text(); })
      .then(function (text) {
        if (known === null) { known = text; return; }
        if (text !== known) {
          sessionStorage.setItem(KEY, String(window.scrollY));
          location.reload();
        }
      })
      .catch(function () { /* 服务器停了：安静等它回来 */ });
  }
  setInterval(poll, 700);
  poll();
})();
</script>"#;

/// 运行中的预览服务器。`drop` 时自动停止并回收线程。
pub struct PreviewServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    /// 产物版本号。每次重新生成后 `bump()` 一下，页面里的脚本据此刷新。
    version: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl PreviewServer {
    /// 在回环地址上启动服务器。`port` 为 0 时由系统分配端口。
    ///
    /// 只绑定 127.0.0.1：预览产物可能包含尚未发布的草稿，不应暴露到局域网。
    pub fn start(root: impl Into<PathBuf>, port: u16) -> Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::InvalidProject(format!(
                "输出目录不存在，请先生成一次: {}",
                root.display()
            )));
        }
        let root = root.canonicalize().map_err(|e| Error::io(&root, e))?;

        let bind = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        let server = tiny_http::Server::http(bind)
            .map_err(|e| Error::Other(format!("预览服务器启动失败（{bind}）: {e}")))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| Error::Other("预览服务器未能取得监听地址".to_string()))?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let version = Arc::new(AtomicU64::new(1));
        let version_ref = version.clone();
        let thread = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match server.recv_timeout(POLL_INTERVAL) {
                    Ok(Some(request)) => {
                        // 一个请求处理时 panic 不能把整个预览线程带走：那之后预览
                        // 端口还在监听但什么都不回，界面上仍写着「运行中」。
                        let handled = crate::util::keep_running("预览请求", || {
                            handle(&root, &version_ref, request)
                        });
                        if let Some(Err(err)) = handled {
                            tracing::warn!("预览请求处理失败: {err}");
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        tracing::warn!("预览服务器接收失败: {err}");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            addr,
            stop,
            version,
            thread: Some(thread),
        })
    }

    /// 告诉浏览器「产物变了」。重新生成之后调用即可，页面会自己刷新。
    pub fn bump(&self) {
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// 站点根地址，如 `http://127.0.0.1:5321`。
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// 拼出某个页面的预览地址。
    pub fn url_for(&self, page_url: &str) -> String {
        format!(
            "{}{}",
            self.base_url(),
            if page_url.starts_with('/') {
                page_url.to_string()
            } else {
                format!("/{page_url}")
            }
        )
    }

    /// 主动停止（等价于 drop）。
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PreviewServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn handle(root: &Path, version: &AtomicU64, request: tiny_http::Request) -> std::io::Result<()> {
    let raw = request.url().to_string();
    let path = raw.split(['?', '#']).next().unwrap_or("/");

    if path == VERSION_PATH {
        let body = version.load(Ordering::Relaxed).to_string();
        return request.respond(text_response(body, "text/plain; charset=utf-8"));
    }

    match resolve(root, path) {
        Some(file) => {
            let mime = mime_for(&file);
            let mut body = std::fs::read(&file)?;
            if mime.starts_with("text/html") {
                body = inject_live_reload(body);
            }
            let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes())
                .expect("MIME 常量应当合法");
            // 预览要立刻反映重新生成的结果，禁止任何缓存。
            let no_cache = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..])
                .expect("Cache-Control 常量应当合法");
            let response = tiny_http::Response::from_data(body)
                .with_header(header)
                .with_header(no_cache);
            request.respond(response)
        }
        None => request.respond(
            tiny_http::Response::from_string(format!("404 Not Found: {path}"))
                .with_status_code(404),
        ),
    }
}

fn text_response(body: String, mime: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes())
        .expect("MIME 应当合法");
    let no_cache = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..])
        .expect("Cache-Control 常量应当合法");
    tiny_http::Response::from_string(body)
        .with_header(header)
        .with_header(no_cache)
}

/// 把自动刷新脚本插到 `</body>` 之前，没有 `</body>` 就追加在末尾。
///
/// 只改内存里的响应体，磁盘上的产物一个字节都不动——`dist/` 是要发布出去的东西。
fn inject_live_reload(body: Vec<u8>) -> Vec<u8> {
    let script = LIVE_RELOAD.replace("__VERSION_PATH__", VERSION_PATH);
    // 不是合法 UTF-8 的「HTML」不改写，原样送回去（拿回所有权，不复制）
    let html = match String::from_utf8(body) {
        Ok(html) => html,
        Err(err) => return err.into_bytes(),
    };
    let injected = match html.rfind("</body>") {
        Some(at) => format!("{}{}{}", &html[..at], script, &html[at..]),
        None => format!("{html}{script}"),
    };
    injected.into_bytes()
}

/// URL → 磁盘路径。返回 `None` 表示 404。
///
/// 三层防护：先剔除 `..` 片段，再拼接，最后校验结果仍在 root 之内。
pub fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path);
    let mut candidate = root.to_path_buf();
    for segment in decoded.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        candidate.push(segment);
    }

    // pretty URL：目录或以 / 结尾时取其 index.html
    if candidate.is_dir() {
        candidate.push("index.html");
    }
    if !candidate.is_file() {
        return None;
    }
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(root).then_some(canonical)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 静态站点会用到的 MIME 类型。定义在 [`crate::util::mime_for`]，内存预览也用同一份。
pub use crate::util::mime_for;

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use super::*;

    fn site() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("posts/hello")).unwrap();
        std::fs::create_dir_all(dir.path().join("css")).unwrap();
        std::fs::write(dir.path().join("index.html"), "<h1>首页</h1>").unwrap();
        std::fs::write(dir.path().join("posts/hello/index.html"), "<h1>文章</h1>").unwrap();
        std::fs::write(dir.path().join("css/main.css"), "body{}").unwrap();
        dir
    }

    /// 极简 HTTP 客户端：发一个 GET，返回整个响应文本。
    fn get(addr: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        String::from_utf8_lossy(&response).to_string()
    }

    #[test]
    fn serves_index_and_pretty_urls() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();
        let addr = server.addr();

        let root = get(addr, "/");
        assert!(root.starts_with("HTTP/1.1 200"), "{root}");
        assert!(root.contains("<h1>首页</h1>"));
        assert!(root.contains("text/html; charset=utf-8"));
        assert!(root.contains("no-store"), "预览响应必须禁用缓存");

        let post = get(addr, "/posts/hello/");
        assert!(post.contains("<h1>文章</h1>"));

        // 不带尾斜杠也应命中目录下的 index.html
        assert!(get(addr, "/posts/hello").contains("<h1>文章</h1>"));
    }

    #[test]
    fn serves_assets_with_correct_mime_and_404s_unknown_paths() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();

        let css = get(server.addr(), "/css/main.css");
        assert!(css.contains("text/css"));

        let missing = get(server.addr(), "/nope.html");
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
    }

    #[test]
    fn query_strings_are_ignored() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();
        assert!(get(server.addr(), "/?v=1#frag").contains("<h1>首页</h1>"));
    }

    #[test]
    fn html_gets_the_live_reload_script_but_assets_do_not() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();

        let page = get(server.addr(), "/");
        assert!(page.contains("data-staticsmith=\"live-reload\""), "{page}");
        assert!(page.contains(VERSION_PATH));
        // 原有内容一个字不少
        assert!(page.contains("<h1>首页</h1>"));

        let css = get(server.addr(), "/css/main.css");
        assert!(!css.contains("live-reload"), "静态资源不该被改写：{css}");
    }

    #[test]
    fn injection_goes_before_the_closing_body_tag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<html><body><p>正文</p></body></html>",
        )
        .unwrap();
        let server = PreviewServer::start(dir.path(), 0).unwrap();

        let page = get(server.addr(), "/");
        let script = page.find("live-reload").unwrap();
        let close = page.find("</body>").unwrap();
        assert!(script < close, "脚本必须在 </body> 之前：{page}");
    }

    #[test]
    fn version_changes_only_after_bump() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();

        let first = get(server.addr(), VERSION_PATH);
        assert!(first.starts_with("HTTP/1.1 200"), "{first}");
        assert_eq!(get(server.addr(), VERSION_PATH), first, "没生成就不该变");

        server.bump();
        assert_ne!(get(server.addr(), VERSION_PATH), first);
    }

    #[test]
    fn non_utf8_html_is_served_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        // 半个 UTF-8 序列：不该因为注入失败而把响应体清空
        std::fs::write(dir.path().join("index.html"), [0x3c, 0x70, 0x3e, 0xff]).unwrap();
        let server = PreviewServer::start(dir.path(), 0).unwrap();

        let page = get(server.addr(), "/");
        assert!(page.contains("<p>"), "{page}");
        assert!(!page.contains("live-reload"));
    }

    #[test]
    fn traversal_attempts_cannot_escape_the_output_dir() {
        let dir = site();
        let outside = dir.path().parent().unwrap().join("secret.txt");
        std::fs::write(&outside, "机密").unwrap();

        let server = PreviewServer::start(dir.path(), 0).unwrap();
        for path in [
            "/../secret.txt",
            "/..%2Fsecret.txt",
            "/posts/../../secret.txt",
        ] {
            let response = get(server.addr(), path);
            assert!(!response.contains("机密"), "{path} 泄露了目录外文件");
        }
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn resolve_maps_urls_to_files() {
        let dir = site();
        let root = dir.path().canonicalize().unwrap();
        assert!(resolve(&root, "/").unwrap().ends_with("index.html"));
        assert!(resolve(&root, "/css/main.css")
            .unwrap()
            .ends_with("main.css"));
        assert!(resolve(&root, "/missing").is_none());
    }

    #[test]
    fn percent_encoded_paths_are_decoded() {
        let dir = site();
        std::fs::create_dir_all(dir.path().join("文章")).unwrap();
        std::fs::write(dir.path().join("文章/index.html"), "中文目录").unwrap();

        let server = PreviewServer::start(dir.path(), 0).unwrap();
        let encoded = "/%E6%96%87%E7%AB%A0/";
        assert!(get(server.addr(), encoded).contains("中文目录"));
    }

    #[test]
    fn missing_output_dir_is_reported_instead_of_panicking() {
        let err = PreviewServer::start("/definitely/not/here", 0)
            .err()
            .expect("目录不存在时应当报错");
        assert!(matches!(err, Error::InvalidProject(_)));
    }

    #[test]
    fn url_for_builds_page_addresses() {
        let dir = site();
        let server = PreviewServer::start(dir.path(), 0).unwrap();
        let expected = format!("{}/posts/hello/", server.base_url());
        assert_eq!(server.url_for("/posts/hello/"), expected);
        assert_eq!(server.url_for("posts/hello/"), expected);
    }

    #[test]
    fn mime_lookup_covers_common_types_and_falls_back() {
        assert_eq!(mime_for(Path::new("a.PNG")), "image/png");
        assert_eq!(mime_for(Path::new("a.woff2")), "font/woff2");
        assert_eq!(mime_for(Path::new("a.unknown")), "application/octet-stream");
    }
}
