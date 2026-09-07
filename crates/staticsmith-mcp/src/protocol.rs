//! JSON-RPC 2.0 与 MCP 方法调度。
//!
//! 这一层不碰 IO，只做「请求 JSON → 响应 JSON」的纯变换，因此协议行为可以完整单测，
//! 不需要真的起服务器或接一个 Agent。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 本服务端实现的协议版本，按新旧顺序排列。
///
/// `initialize` 时如果客户端请求的版本在这个列表里就原样回应，否则回落到第一个。
/// 目前主流客户端仍大量使用 2024-11-05（旧 HTTP+SSE 传输），因此必须继续兼容。
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2025-06-18", "2025-03-26", "2024-11-05", "2025-11-25"];

pub const SERVER_NAME: &str = "staticsmith";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 给 Agent 的使用提示，随 `initialize` 返回。
pub const INSTRUCTIONS: &str = "\
StaticSmith 是本地静态网站生成器。典型流程：\
先用 list_pages 了解站点结构，用 read_content 读原文（含 +++ TOML front matter），\
修改后用 write_content 写回，再用 build_plan 确认影响范围、build_site 生成产物。\
修改 templates/components/*.html 会级联影响所有引用它的页面，改动前先看 build_plan。\
写入类与发布类工具默认关闭，需要服务端以 --allow-write / --allow-deploy 启动。";

/// JSON-RPC 标准错误码。
pub mod error_code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

/// 一条进来的 JSON-RPC 消息。
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    /// 通知（notification）没有 id，不需要回响应。
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// 构造成功响应。
pub fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 构造错误响应。
pub fn failure(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

/// 协商协议版本。
pub fn negotiate_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some(v) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .find(|s| **s == v)
            .copied()
            .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0]),
        None => SUPPORTED_PROTOCOL_VERSIONS[0],
    }
}

/// `initialize` 的结果体。
pub fn initialize_result(protocol_version: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            // 只提供工具；资源与提示词暂未实现，如实声明能力，避免客户端做无效调用。
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "本地静站·工坊",
            "version": SERVER_VERSION
        },
        "instructions": INSTRUCTIONS
    })
}

/// 把工具执行结果包装成 MCP 的 `tools/call` 结果。
///
/// 注意：工具执行失败用 `isError: true` 的正常结果表达，而不是 JSON-RPC 错误——
/// 前者 Agent 能看到失败原因并自行重试，后者通常被客户端当成传输故障。
pub fn tool_result(text: impl Into<String>, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": is_error
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_known_versions_and_falls_back() {
        assert_eq!(negotiate_version(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate_version(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate_version(Some("1999-01-01")), "2025-06-18");
        assert_eq!(negotiate_version(None), "2025-06-18");
    }

    #[test]
    fn initialize_result_declares_only_implemented_capabilities() {
        let result = initialize_result("2025-06-18");
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert!(result["capabilities"]["tools"].is_object());
        assert!(
            result["capabilities"]["resources"].is_null(),
            "未实现的能力不应声明"
        );
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn requests_without_id_are_notifications() {
        let notification: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(notification.id.is_none());

        let call: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#).unwrap();
        assert_eq!(call.id, Some(json!(7)));
    }

    #[test]
    fn tool_result_marks_failures_inside_the_result() {
        let ok = tool_result("done", false);
        assert_eq!(ok["isError"], false);
        assert_eq!(ok["content"][0]["type"], "text");

        let err = tool_result("boom", true);
        assert_eq!(err["isError"], true);
    }
}
