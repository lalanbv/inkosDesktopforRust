//! JSON-RPC 2.0 协议实现

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub method: String,
    pub params: Option<Value>,
}

impl RpcRequest {
    /// 创建新请求
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// 创建通知（无 ID，不需要响应）
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// 创建成功响应
    pub fn success(id: u64, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// 创建错误响应
    pub fn error(id: u64, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// 标准错误码
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // 自定义错误码（-32000 到 -32099）
    pub const PERMISSION_DENIED: i32 = -32000;
    pub const RESOURCE_NOT_FOUND: i32 = -32001;
    pub const TIMEOUT: i32 = -32002;

    /// 创建错误
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// 带附加数据的错误
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_serialization() {
        let req = RpcRequest::new(1, "test.method", Some(serde_json::json!({"key": "value"})));
        let json = serde_json::to_string(&req).unwrap();

        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"test.method\""));
    }

    #[test]
    fn test_rpc_notification() {
        let notif = RpcRequest::notification("test.event", None);
        let json = serde_json::to_string(&notif).unwrap();

        // Notification 应该包含 id: null
        assert!(json.contains("\"id\":null") || json.contains("\"id\": null"));
    }

    #[test]
    fn test_rpc_response_success() {
        let resp = RpcResponse::success(1, serde_json::json!({"status": "ok"}));
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_rpc_response_error() {
        let resp = RpcResponse::error(1, RpcError::new(RpcError::METHOD_NOT_FOUND, "Method not found"));
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"error\""));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn test_rpc_error_codes() {
        assert_eq!(RpcError::PARSE_ERROR, -32700);
        assert_eq!(RpcError::PERMISSION_DENIED, -32000);
    }
}
