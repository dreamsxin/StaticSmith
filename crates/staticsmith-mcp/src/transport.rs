//! 两种传输：stdio 与 HTTP。
//!
//! HTTP 端提供三个路径：
//!
//! - `POST /mcp`：Streamable HTTP 的最简形态，请求体是 JSON-RPC，响应直接是 JSON
//! - `GET /sse`：旧版 HTTP+SSE 传输的服务端流，首个事件 `endpoint` 告诉客户端往哪 POST
//! - `POST /messages?sessionId=…`：旧版传输的客户端上行，响应经 SSE 流推回
//!
//! 旧端点已被规范标记为 deprecated，但现存客户端里仍有大量只实现了它，
//! 所以两套并存——规范本身也是这么建议做向后兼容的。

use std::collections::HashMap;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use staticsmith_core::error::{Error, Result};
use tiny_http::{Header, Request, Response, StatusCode};

use crate::McpServer;

/// 接收循环的轮询间隔，决定停止信号的生效延迟。
const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// SSE 空闲时发送注释行的间隔，避免中间代理判定连接超时。
const SSE_KEEPALIVE: Duration = Duration::from_secs(15);

type Sessions = Arc<Mutex<HashMap<String, Sender<Vec<u8>>>>>;

/// stdio 传输：一行一条 JSON-RPC 消息，读到 EOF 结束。
///
/// 客户端（Claude Desktop、Cursor 等）会把本进程作为子进程拉起，
/// 因此**不能**往 stdout 写任何非协议内容——日志一律走 stderr。
pub fn serve_stdio(server: &McpServer) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_text(&line) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// 运行中的 HTTP 传输。`drop` 时停止接收并回收线程。
pub struct McpHttpServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// 留一份服务端引用，只为回答「现在开的是什么权限」。
    ///
    /// 桌面端要在界面上显示这一点，而权限是启动参数：让调用方自己记着的话，
    /// 迟早出现「界面显示可写、服务端其实只读」。状态的唯一来源应该是正在跑的那个服务端。
    server: Arc<McpServer>,
}

impl McpHttpServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn permissions(&self) -> crate::Permissions {
        self.server.permissions()
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// 供客户端配置使用的两个地址。
    pub fn mcp_endpoint(&self) -> String {
        format!("{}/mcp", self.base_url())
    }

    pub fn sse_endpoint(&self) -> String {
        format!("{}/sse", self.base_url())
    }

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

impl Drop for McpHttpServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 在回环地址上启动 HTTP 传输。`port` 为 0 时由系统分配。
///
/// 只绑 127.0.0.1：这个端点能读写站点源文件，绝不该暴露到局域网。
/// 需要远程访问时请自己套一层带认证的反向代理。
pub fn serve_http(server: Arc<McpServer>, port: u16) -> Result<McpHttpServer> {
    let bind = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let http = tiny_http::Server::http(bind)
        .map_err(|e| Error::Other(format!("MCP 服务器启动失败（{bind}）: {e}")))?;
    let addr = http
        .server_addr()
        .to_ip()
        .ok_or_else(|| Error::Other("MCP 服务器未能取得监听地址".to_string()))?;

    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let kept = server.clone();

    let thread = std::thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
            match http.recv_timeout(POLL_INTERVAL) {
                Ok(Some(request)) => {
                    let server = server.clone();
                    let sessions = sessions.clone();
                    // SSE 连接是长连接，必须离开接收循环，否则后续请求全被阻塞。
                    std::thread::spawn(move || route(request, server, sessions));
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("MCP 服务器接收失败: {e}");
                    break;
                }
            }
        }
    });

    Ok(McpHttpServer {
        addr,
        stop,
        thread: Some(thread),
        server: kept,
    })
}

fn route(mut request: Request, server: Arc<McpServer>, sessions: Sessions) {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    // 浏览器发来的请求一概不认，无论走哪个端点。
    //
    // 这个端口没有鉴权，唯一的边界就是「谁能发出这个请求」。MCP 客户端是本机进程，
    // 不会带 `Origin`；网页里的 `fetch` / `EventSource` 一定会带。所以「带了 Origin」
    // 等价于「这是网页发来的」，直接 403。放行「本机来源的 Origin」是没有意义的——
    // 恶意页面完全可以由本机的另一个服务托管。
    if let Some(origin) = header_value(&request, "Origin") {
        let body = format!("拒绝网页发起的请求（Origin: {origin}）。MCP 客户端应当是本机进程。");
        respond(request, StatusCode(403), body);
        return;
    }

    let outcome = match (method.as_str(), path.as_str()) {
        ("POST", "/mcp") => handle_streamable(&mut request, &server),
        ("POST", "/messages") => handle_legacy_message(&mut request, &url, &server, &sessions),
        ("GET", "/sse") => return handle_sse(request, &sessions),
        ("GET", "/mcp") => Err((
            405,
            "本服务端不在 /mcp 上提供 GET 流，请改用 POST /mcp 或 GET /sse".to_string(),
        )),
        ("GET", "/") => Ok(Some(usage_text(&server))),
        _ => Err((404, format!("未知路径: {path}"))),
    };

    let response = match outcome {
        Ok(Some(body)) => Response::from_string(body)
            .with_header(json_header())
            .with_status_code(StatusCode(200)),
        // 只有通知，没有需要返回的响应体。
        Ok(None) => Response::from_string("").with_status_code(StatusCode(202)),
        Err((code, message)) => Response::from_string(message).with_status_code(StatusCode(code)),
    };
    if let Err(e) = request.respond(response) {
        tracing::debug!("MCP 响应发送失败: {e}");
    }
}

fn respond(request: Request, code: StatusCode, body: String) {
    if let Err(e) = request.respond(Response::from_string(body).with_status_code(code)) {
        tracing::debug!("MCP 响应发送失败: {e}");
    }
}

/// `POST /mcp`：请求体里的 JSON-RPC 直接同步应答。
fn handle_streamable(
    request: &mut Request,
    server: &McpServer,
) -> std::result::Result<Option<String>, (u16, String)> {
    let body = read_body(request)?;
    Ok(server.handle_text(&body))
}

/// `POST /messages?sessionId=…`：响应通过对应的 SSE 流推回，本次 HTTP 只回 202。
fn handle_legacy_message(
    request: &mut Request,
    url: &str,
    server: &McpServer,
    sessions: &Sessions,
) -> std::result::Result<Option<String>, (u16, String)> {
    let session_id =
        query_param(url, "sessionId").ok_or((400, "缺少 sessionId 查询参数".to_string()))?;
    let body = read_body(request)?;

    let sender = sessions
        .lock()
        .expect("会话表锁被污染")
        .get(&session_id)
        .cloned()
        .ok_or((404, format!("会话不存在或已断开: {session_id}")))?;

    if let Some(response) = server.handle_text(&body) {
        let event = format!("event: message\ndata: {response}\n\n");
        if sender.send(event.into_bytes()).is_err() {
            return Err((410, "SSE 流已关闭".to_string()));
        }
    }
    Ok(None)
}

/// `GET /sse`：建立服务端事件流，首个事件告诉客户端上行地址。
///
/// 这里绕开 `Request::respond` 手写响应：tiny_http 的 `Response` + `Read` 组合
/// 会等读端产出足够数据才把字节推出去，而 SSE 要求每条事件立刻可见，
/// 所以自己拿 writer 并在每条事件后 flush。
fn handle_sse(request: Request, sessions: &Sessions) {
    let session_id = new_session_id();
    let (tx, rx) = channel::<Vec<u8>>();

    // 先放进会话表再回 endpoint 事件，避免客户端太快 POST 时查不到会话。
    sessions
        .lock()
        .expect("会话表锁被污染")
        .insert(session_id.clone(), tx.clone());

    let mut writer = request.into_writer();
    // 不带 Content-Length，正文由连接关闭界定，这是 SSE 的常规做法。
    let head = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream\r\n\
                Cache-Control: no-store\r\n\
                Connection: close\r\n\
                \r\n";
    let mut alive = writer.write_all(head.as_bytes()).is_ok() && writer.flush().is_ok();

    let endpoint = format!("event: endpoint\ndata: /messages?sessionId={session_id}\n\n");
    if alive {
        alive = writer.write_all(endpoint.as_bytes()).is_ok() && writer.flush().is_ok();
    }

    while alive {
        let payload = match rx.recv_timeout(SSE_KEEPALIVE) {
            Ok(bytes) => bytes,
            // 空闲时发一条 SSE 注释，维持连接。
            Err(RecvTimeoutError::Timeout) => b": keepalive\n\n".to_vec(),
            // 所有发送端已释放：流结束。
            Err(RecvTimeoutError::Disconnected) => break,
        };
        alive = writer.write_all(&payload).is_ok() && writer.flush().is_ok();
    }

    sessions.lock().expect("会话表锁被污染").remove(&session_id);
    tracing::debug!("SSE 会话结束: {session_id}");
}

fn read_body(request: &mut Request) -> std::result::Result<String, (u16, String)> {
    // 必须是 JSON。`text/plain` 属于 CORS「简单请求」，**不触发预检**——恶意网页可以
    // 朝 127.0.0.1 的每个端口盲打而浏览器不拦（虽然读不到响应，但写操作已经发生了）。
    // 要求 application/json 之后浏览器必须先发 OPTIONS 预检，而这里从不放行预检。
    // 这一条与 `route` 里的 Origin 检查是两道独立的门，缺任一条都能被绕过。
    match header_value(request, "Content-Type") {
        Some(ct) if ct.to_ascii_lowercase().contains("application/json") => {}
        Some(ct) => return Err((415, format!("请求体必须是 application/json，收到：{ct}"))),
        None => return Err((415, "缺少 Content-Type: application/json".to_string())),
    }

    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|e| (400, format!("读取请求体失败: {e}")))?;
    Ok(body)
}

/// 取一个请求头的值。`name` 要求 `'static`：`HeaderField::equiv` 只接受静态字符串，
/// 而调用点全是字面量。
fn header_value(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

fn query_param(url: &str, key: &str) -> Option<String> {
    url.split_once('?')?
        .1
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
}

/// 会话 id：进程启动时刻的纳秒 + 自增序号做哈希。
///
/// 它只用于把上行 POST 关联到某条 SSE 流，不是安全凭证——服务端本身只监听回环地址。
fn new_session_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    staticsmith_core::util::hash_str(&format!("{nanos}-{n}"))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("头部常量应当合法")
}

fn json_header() -> Header {
    header("Content-Type", "application/json; charset=utf-8")
}

fn usage_text(server: &McpServer) -> String {
    format!(
        "StaticSmith MCP 服务端（{}）\n\n\
         POST /mcp                      Streamable HTTP\n\
         GET  /sse                      旧版 HTTP+SSE 服务端流\n\
         POST /messages?sessionId=…     旧版 HTTP+SSE 客户端上行\n",
        server.permissions().summary()
    )
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::net::TcpStream;

    use serde_json::Value;
    use staticsmith_core::scaffold;

    use super::*;
    use crate::Permissions;

    fn start() -> (tempfile::TempDir, McpHttpServer) {
        let dir = tempfile::tempdir().unwrap();
        scaffold::init_project(dir.path(), Some("MCP 传输测试"), scaffold::Preset::Docs).unwrap();
        let server = Arc::new(McpServer::open(dir.path(), Permissions::read_only()).unwrap());
        let http = serve_http(server, 0).unwrap();
        (dir, http)
    }

    fn connect(addr: SocketAddr) -> TcpStream {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }

    /// 发一个完整请求并读回全部响应（Connection: close）。
    fn request(addr: SocketAddr, raw: &str) -> String {
        let mut stream = connect(addr);
        stream.write_all(raw.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(&response).to_string()
    }

    fn post(addr: SocketAddr, path: &str, body: &str) -> String {
        request(
            addr,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// 从流里持续读，直到出现 needle 或超时。
    fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut accumulated = String::new();
        let mut chunk = [0u8; 1024];
        for _ in 0..200 {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    accumulated.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    if accumulated.contains(needle) {
                        return accumulated;
                    }
                }
                Err(_) => break,
            }
        }
        accumulated
    }

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}"#;

    /// 带自定义头的 POST，用来模拟浏览器发来的请求。
    fn post_with(addr: SocketAddr, path: &str, extra: &str, body: &str) -> String {
        request(
            addr,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\n{extra}\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// 带 `Origin` 就说明是网页发来的，一律 403。
    ///
    /// 这个端口没有鉴权，用户开着 MCP 时任意网页都能朝本机端口发请求；
    /// 挡住 Origin 是唯一不需要用户配置的边界。
    #[test]
    fn requests_from_web_pages_are_refused() {
        let (_dir, http) = start();

        let response = post_with(
            http.addr(),
            "/mcp",
            "Origin: https://evil.example\r\nContent-Type: application/json\r\n",
            INITIALIZE,
        );

        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    }

    /// `text/plain` 是 CORS 简单请求，不触发预检——必须堵掉。
    #[test]
    fn non_json_bodies_are_refused() {
        let (_dir, http) = start();

        let plain = post_with(
            http.addr(),
            "/mcp",
            "Content-Type: text/plain\r\n",
            INITIALIZE,
        );
        assert!(plain.starts_with("HTTP/1.1 415"), "{plain}");

        let missing = post_with(http.addr(), "/mcp", "", INITIALIZE);
        assert!(missing.starts_with("HTTP/1.1 415"), "{missing}");
    }

    #[test]
    fn streamable_endpoint_answers_with_json() {
        let (_dir, http) = start();
        let response = post(http.addr(), "/mcp", INITIALIZE);

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("application/json"));
        let body = response.split("\r\n\r\n").nth(1).unwrap();
        let json: Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(json["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn notifications_get_202_with_no_body() {
        let (_dir, http) = start();
        let response = post(
            http.addr(),
            "/mcp",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
    }

    #[test]
    fn tools_list_over_http_respects_permissions() {
        let (_dir, http) = start();
        let response = post(
            http.addr(),
            "/mcp",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        );
        assert!(response.contains("list_pages"));
        assert!(
            !response.contains("write_content"),
            "只读模式不应暴露写入工具"
        );
    }

    #[test]
    fn legacy_sse_handshake_delivers_responses_on_the_stream() {
        let (_dir, http) = start();

        // 1. 打开 SSE 流，拿到 endpoint 事件里的上行地址。
        let mut sse = connect(http.addr());
        sse.write_all(b"GET /sse HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
            .unwrap();
        let handshake = read_until(&mut sse, "sessionId=");
        assert!(handshake.contains("text/event-stream"), "{handshake}");
        assert!(handshake.contains("event: endpoint"), "{handshake}");

        let session_id = handshake
            .split("sessionId=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_string();

        // 2. 上行 POST 只拿到 202，真正的响应走 SSE。
        let ack = post(
            http.addr(),
            &format!("/messages?sessionId={session_id}"),
            INITIALIZE,
        );
        assert!(ack.starts_with("HTTP/1.1 202"), "{ack}");

        // 3. 从 SSE 流里读到 message 事件。
        let event = read_until(&mut sse, "protocolVersion");
        assert!(event.contains("event: message"), "{event}");
        assert!(event.contains("2024-11-05"), "{event}");
    }

    #[test]
    fn unknown_session_is_rejected() {
        let (_dir, http) = start();
        let response = post(http.addr(), "/messages?sessionId=nope", INITIALIZE);
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    }

    #[test]
    fn missing_session_id_is_a_bad_request() {
        let (_dir, http) = start();
        let response = post(http.addr(), "/messages", INITIALIZE);
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    }

    #[test]
    fn get_on_mcp_endpoint_returns_405_and_root_documents_usage() {
        let (_dir, http) = start();

        let not_allowed = request(
            http.addr(),
            "GET /mcp HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(not_allowed.starts_with("HTTP/1.1 405"), "{not_allowed}");

        let root = request(
            http.addr(),
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(root.contains("POST /mcp"));
        assert!(root.contains("只读模式"));
    }

    #[test]
    fn unknown_paths_are_404() {
        let (_dir, http) = start();
        let response = request(
            http.addr(),
            "GET /nope HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    }

    #[test]
    fn endpoints_expose_their_urls_for_client_config() {
        let (_dir, http) = start();
        assert!(http.mcp_endpoint().ends_with("/mcp"));
        assert!(http.sse_endpoint().ends_with("/sse"));
        assert!(http.base_url().starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn session_ids_are_unique() {
        let a = new_session_id();
        let b = new_session_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn query_params_are_parsed() {
        assert_eq!(
            query_param("/messages?sessionId=abc&x=1", "sessionId"),
            Some("abc".to_string())
        );
        assert_eq!(query_param("/messages", "sessionId"), None);
    }
}
