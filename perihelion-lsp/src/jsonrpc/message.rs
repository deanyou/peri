use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 请求 ID
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

/// JSON-RPC 请求（有 id，期望响应）
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: i64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: RequestId::Number(id),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<RequestId>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 错误
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// JSON-RPC 通知（无 id，不期望响应）
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(1, "initialize", Some(serde_json::json!({"test": true})));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""jsonrpc":"2.0""#));
        assert!(json.contains(r#""id":1"#));
        assert!(json.contains(r#""method":"initialize""#));
        assert!(json.contains(r#""test":true"#));
    }

    #[test]
    fn test_notification_serialization() {
        let notif = JsonRpcNotification::new("initialized", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains(r#""jsonrpc":"2.0""#));
        assert!(!json.contains(r#""id""#));
        assert!(json.contains(r#""method":"initialized""#));
    }

    #[test]
    fn test_response_deserialization_success() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_response_deserialization_error() {
        let json =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn test_response_deserialization_notification_response() {
        // 某些 LSP 服务器对通知也返回无 id 的响应
        let json = r#"{"jsonrpc":"2.0","result":null}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.id.is_none());
    }
}
