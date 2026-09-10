//! StaticSmith 的 MCP（Model Context Protocol）服务端。
//!
//! 让 AI Agent 用工具调用的方式操作站点：读内容、改模板、看级联影响、生成、发布。
//!
//! ## 传输
//!
//! - **stdio**：客户端拉起子进程，最省事，Claude Desktop / Cursor 之类默认走这条
//! - **HTTP**：`POST /mcp` 直接返回 JSON（Streamable HTTP 的最简形态），
//!   同时保留 `GET /sse` + `POST /messages?sessionId=` 这对旧端点（2024-11-05 HTTP+SSE），
//!   因为现存客户端里仍有大量只实现了旧传输的
//!
//! ## 权限
//!
//! 默认只读。写入与发布必须显式开启（[`Permissions`]），并且 `tools/list` 只列出
//! 当前允许的工具——Agent 看不到用不了的工具，就不会反复试错。
//!
//! ```no_run
//! use staticsmith_mcp::{McpServer, Permissions};
//!
//! let server = McpServer::open("./my-site", Permissions::read_only())?;
//! staticsmith_mcp::transport::serve_stdio(&server)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::path::Path;
use std::sync::Mutex;

use serde_json::{json, Value};
use staticsmith_core::Builder;

pub mod protocol;
pub mod tools;
pub mod transport;

pub use tools::Permissions;
pub use transport::{serve_http, serve_stdio, McpHttpServer};

use protocol::{error_code, failure, success, Request};

/// MCP 服务端。内部持有一个 [`Builder`]，串行处理请求。
///
/// 串行是刻意的：构建引擎会重载模板与内容，两个请求同时改同一份状态没有意义，
/// 而 Agent 的调用本身就是一问一答。
pub struct McpServer {
    builder: Mutex<Builder>,
    permissions: Permissions,
}

impl McpServer {
    /// 打开项目。
    pub fn open(
        project_root: impl AsRef<Path>,
        permissions: Permissions,
    ) -> staticsmith_core::Result<Self> {
        Ok(Self {
            builder: Mutex::new(Builder::open(project_root)?),
            permissions,
        })
    }

    pub fn permissions(&self) -> Permissions {
        self.permissions
    }

    /// 处理一条原始 JSON 文本，返回要回给客户端的文本。
    ///
    /// 返回 `None` 表示这条消息是通知（notification），按 JSON-RPC 规范不回响应。
    pub fn handle_text(&self, text: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(e) => {
                return Some(
                    failure(
                        Value::Null,
                        error_code::PARSE_ERROR,
                        format!("JSON 解析失败: {e}"),
                    )
                    .to_string(),
                )
            }
        };
        self.handle_value(value).map(|v| v.to_string())
    }

    /// 处理一条已解析的消息。支持 JSON-RPC 批量数组。
    pub fn handle_value(&self, value: Value) -> Option<Value> {
        if let Value::Array(items) = value {
            let responses: Vec<Value> = items
                .into_iter()
                .filter_map(|item| self.handle_value(item))
                .collect();
            return (!responses.is_empty()).then_some(Value::Array(responses));
        }

        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(e) => {
                return Some(failure(
                    Value::Null,
                    error_code::INVALID_REQUEST,
                    format!("不是合法的 JSON-RPC 请求: {e}"),
                ))
            }
        };

        let id = request.id.clone();
        let result = self.dispatch(&request);

        match (id, result) {
            // 通知：无论处理结果如何都不回响应。
            (None, _) => None,
            (Some(id), Ok(result)) => Some(success(id, result)),
            (Some(id), Err((code, message))) => Some(failure(id, code, message)),
        }
    }

    fn dispatch(&self, request: &Request) -> Result<Value, (i64, String)> {
        match request.method.as_str() {
            "initialize" => {
                let requested = request
                    .params
                    .get("protocolVersion")
                    .and_then(Value::as_str);
                Ok(protocol::initialize_result(protocol::negotiate_version(
                    requested,
                )))
            }

            // 客户端握手完成的通知，无需处理。
            "notifications/initialized" | "notifications/cancelled" => Ok(json!({})),

            "ping" => Ok(json!({})),

            "tools/list" => Ok(tools::list_json(self.permissions)),

            "tools/call" => {
                let name = request
                    .params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((error_code::INVALID_PARAMS, "缺少 name".to_string()))?;
                let args = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                // 锁被污染说明上一次持锁时有人 panic 了，不代表 Builder 不可用——
                // 它是一份从磁盘重读就能恢复的东西。旧写法在这里永久返回
                // 「内部状态锁异常」：端点还开着、界面还写着运行中，而 Agent 的每个
                // 请求都失败，看不出原因。
                let mut builder = match self.builder.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        tracing::warn!("状态锁曾在 panic 中被污染，重载项目后继续");
                        let mut guard = poisoned.into_inner();
                        if let Err(err) = guard.reload() {
                            tracing::error!("污染恢复时重载项目失败: {err}");
                        }
                        guard
                    }
                };

                // 工具里的 panic 变成一条 JSON-RPC 错误。穿出去的话，HTTP 那侧会掀掉
                // tiny_http 的工作线程、stdio 那侧会掀掉主循环，端点静默停止服务。
                match catch_tool_panic(|| tools::call(&mut builder, self.permissions, name, &args))
                {
                    Ok(result) => Ok(result),
                    Err(message) => {
                        tracing::error!("工具 {name} 内部 panic：{message}");
                        // panic 可能把 Builder 停在改了一半的状态上，拿着半套内存状态
                        // 继续生成产物比报错更糟。
                        if let Err(err) = builder.reload() {
                            tracing::error!("panic 后重载项目失败: {err}");
                        }
                        Err((
                            error_code::INTERNAL_ERROR,
                            format!("工具 {name} 内部错误：{message}。这是程序缺陷，请附日志反馈；项目状态已从磁盘重新载入"),
                        ))
                    }
                }
            }

            other => Err((
                error_code::METHOD_NOT_FOUND,
                format!("不支持的方法: {other}"),
            )),
        }
    }
}

/// 兜住工具执行里的 panic，`Err` 带回 panic 消息。
///
/// 为什么必须有：HTTP 那侧每个请求跑在 tiny_http 的工作线程上，stdio 那侧就是主循环。
/// 任何一处 panic 穿出去，端点就静默停止服务——界面上还写着「运行中」，
/// Agent 那边只看到连接断了。翻译成一条 JSON-RPC 内部错误，Agent 能读到原因，
/// 端点继续活着。
///
/// 与桌面端 `state::catch_panics` 同一套思路，各自实现是因为两边错误类型不同，
/// 而这段逻辑只有五行——为它拉一个公共 crate 不值得。
fn catch_tool_panic<T>(f: impl FnOnce() -> T) -> std::result::Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
        if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<非字符串 payload>".to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use staticsmith_core::scaffold;

    use super::*;

    fn server(permissions: Permissions) -> (tempfile::TempDir, McpServer) {
        let dir = tempfile::tempdir().unwrap();
        scaffold::init_project(dir.path(), Some("MCP 测试站"), scaffold::Preset::Docs).unwrap();
        let server = McpServer::open(dir.path(), permissions).unwrap();
        (dir, server)
    }

    fn call(server: &McpServer, name: &str, args: Value) -> Value {
        let response = server
            .handle_value(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": name, "arguments": args }
            }))
            .expect("tools/call 应当有响应");
        response["result"].clone()
    }

    fn text_of(result: &Value) -> String {
        result["content"][0]["text"].as_str().unwrap().to_string()
    }

    /// 锁被污染之后端点不能就此变成废的。
    ///
    /// 「污染」意味着上一次持锁时有人 panic 了。旧写法把它翻译成
    /// 「内部状态锁异常」并**永久**这样回下去：Agent 从此每个请求都失败，
    /// 而端点还开着、界面上还显示「运行中」，看不出发生了什么。
    /// 里面装的是 `Builder`，一份从磁盘重读就能恢复的东西。
    #[test]
    fn a_poisoned_builder_lock_does_not_kill_the_endpoint() {
        let (_dir, server) = server(Permissions::read_only());

        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = server.builder.lock().unwrap();
            panic!("模拟持锁期间的 panic");
        }));
        assert!(poisoned.is_err());
        assert!(server.builder.is_poisoned(), "这条测试的前提是锁已被污染");

        let result = call(&server, "site_info", json!({}));
        assert!(text_of(&result).contains("MCP 测试站"));
    }

    /// panic 要变成一条 JSON-RPC 错误，而不是掀掉工作线程。
    ///
    /// HTTP 那侧每个请求跑在 tiny_http 的工作线程上，stdio 那侧就是主循环：
    /// 任何一处 panic 穿出去，端点就静默停止服务。这里测的是兜底本身，
    /// 接线只有 `dispatch` 里那一行。
    #[test]
    fn a_panicking_call_becomes_an_error_not_a_dead_thread() {
        assert_eq!(catch_tool_panic(|| 7), Ok(7));
        let caught = catch_tool_panic(|| panic!("工具内部炸了"));
        assert_eq!(caught, Err("工具内部炸了".to_string()));
    }

    #[test]
    fn initialize_echoes_a_supported_protocol_version() {
        let (_dir, server) = server(Permissions::read_only());
        let response = server
            .handle_value(json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "initialize",
                "params": { "protocolVersion": "2024-11-05", "capabilities": {} }
            }))
            .unwrap();

        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "staticsmith");
    }

    #[test]
    fn notifications_get_no_response() {
        let (_dir, server) = server(Permissions::read_only());
        assert!(server
            .handle_value(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .is_none());
    }

    #[test]
    fn malformed_json_becomes_a_parse_error() {
        let (_dir, server) = server(Permissions::read_only());
        let response: Value =
            serde_json::from_str(&server.handle_text("{ not json").unwrap()).unwrap();
        assert_eq!(response["error"]["code"], error_code::PARSE_ERROR);
    }

    #[test]
    fn unknown_methods_return_method_not_found() {
        let (_dir, server) = server(Permissions::read_only());
        let response = server
            .handle_value(json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }))
            .unwrap();
        assert_eq!(response["error"]["code"], error_code::METHOD_NOT_FOUND);
    }

    #[test]
    fn batched_requests_return_batched_responses() {
        let (_dir, server) = server(Permissions::read_only());
        let response = server
            .handle_value(json!([
                { "jsonrpc": "2.0", "id": 1, "method": "ping" },
                { "jsonrpc": "2.0", "method": "notifications/initialized" },
                { "jsonrpc": "2.0", "id": 2, "method": "tools/list" }
            ]))
            .unwrap();

        let items = response.as_array().unwrap();
        assert_eq!(items.len(), 2, "通知不占响应位");
        assert_eq!(items[0]["id"], 1);
        assert!(items[1]["result"]["tools"].is_array());
    }

    #[test]
    fn read_tools_work_on_a_fresh_project() {
        let (_dir, server) = server(Permissions::read_only());

        let info = text_of(&call(&server, "site_info", json!({})));
        assert!(info.contains("MCP 测试站"));

        let pages = text_of(&call(&server, "list_pages", json!({ "section": "posts" })));
        assert!(pages.contains("posts/hello-staticsmith.md"));

        let raw = text_of(&call(
            &server,
            "read_content",
            json!({ "source": "posts/hello-staticsmith.md" }),
        ));
        assert!(raw.starts_with("+++"), "应返回含 front matter 的原文");

        let hits = text_of(&call(
            &server,
            "search_content",
            json!({ "query": "依赖图" }),
        ));
        assert!(hits.contains("posts/hello-staticsmith.md"));

        let templates = text_of(&call(&server, "list_templates", json!({})));
        assert!(templates.contains("components/header.html"));
    }

    #[test]
    fn write_tools_are_denied_in_read_only_mode() {
        let (dir, server) = server(Permissions::read_only());
        let result = call(
            &server,
            "write_content",
            json!({ "source": "posts/x.md", "raw": "+++\ntitle=\"x\"\n+++\n" }),
        );

        assert_eq!(result["isError"], true);
        assert!(text_of(&result).contains("需要更高权限"));
        assert!(
            !dir.path().join("content/posts/x.md").exists(),
            "不应写入任何文件"
        );
    }

    #[test]
    fn write_content_reports_the_cascade_plan() {
        let (dir, server) = server(Permissions {
            write: true,
            deploy: false,
        });

        let created = text_of(&call(
            &server,
            "create_content",
            json!({ "title": "Agent 写的文章", "section": "posts", "draft": false }),
        ));
        assert!(created.contains("posts/agent-写的文章.md"), "{created}");

        let written = text_of(&call(
            &server,
            "write_content",
            json!({
                "source": "posts/agent-写的文章.md",
                "raw": "+++\ntitle = \"改过标题\"\n+++\n\n正文\n"
            }),
        ));
        assert!(written.contains("\"plan\""));
        assert!(dir.path().join("content/posts/agent-写的文章.md").is_file());

        let report = text_of(&call(&server, "build_site", json!({ "mode": "full" })));
        assert!(report.contains("pages_rendered"));
        assert!(dir.path().join("dist/index.html").is_file());
    }

    #[test]
    fn template_writes_cannot_escape_the_template_dir() {
        let (dir, server) = server(Permissions {
            write: true,
            deploy: false,
        });

        let result = call(
            &server,
            "write_template",
            json!({ "name": "../../evil.html", "source": "x" }),
        );

        assert_eq!(result["isError"], false, "路径被清洗后写入是允许的");
        assert!(dir.path().join("templates/evil.html").is_file());
        assert!(!dir.path().parent().unwrap().join("evil.html").exists());
    }

    #[test]
    fn editing_a_global_component_shows_the_affected_pages() {
        let (_dir, server) = server(Permissions {
            write: true,
            deploy: false,
        });
        call(&server, "build_site", json!({ "mode": "full" }));

        let header = text_of(&call(
            &server,
            "read_template",
            json!({ "name": "components/header.html" }),
        ));
        let written = text_of(&call(
            &server,
            "write_template",
            json!({
                "name": "components/header.html",
                "source": header.replace("首页", "回到首页")
            }),
        ));

        assert!(
            written.contains("layouts/base.html"),
            "应列出级联影响的模板"
        );
        assert!(written.contains("affected_templates"));
    }

    #[test]
    fn deploy_requires_its_own_permission() {
        let (_dir, server) = server(Permissions {
            write: true,
            deploy: false,
        });
        let result = call(&server, "deploy_site", json!({}));
        assert_eq!(result["isError"], true);
        assert!(text_of(&result).contains("--allow-deploy"));
    }

    #[test]
    fn unknown_tool_is_an_in_result_error() {
        let (_dir, server) = server(Permissions::read_only());
        let result = call(&server, "no_such_tool", json!({}));
        assert_eq!(result["isError"], true);
        assert!(text_of(&result).contains("未知工具"));
    }

    #[test]
    fn missing_required_argument_is_reported_to_the_agent() {
        let (_dir, server) = server(Permissions::read_only());
        let result = call(&server, "read_content", json!({}));
        assert_eq!(result["isError"], true);
        assert!(text_of(&result).contains("source"));
    }
}
