use lsp_types::*;
use serde_json::Value;

use crate::jsonrpc::JsonRpcRequest;

/// 构建 goToDefinition 请求
pub fn goto_definition_request(
    id: i64,
    text_document_position: TextDocumentPositionParams,
) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/definition",
        Some(serde_json::to_value(text_document_position).unwrap_or(Value::Null)),
    )
}

/// 构建 findReferences 请求
pub fn find_references_request(id: i64, params: ReferenceParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/references",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 hover 请求
pub fn hover_request(id: i64, params: HoverParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/hover",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 documentSymbol 请求
pub fn document_symbol_request(id: i64, params: DocumentSymbolParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/documentSymbol",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 workspaceSymbol 请求
pub fn workspace_symbol_request(id: i64, params: WorkspaceSymbolParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "workspace/symbol",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 goToImplementation 请求
pub fn goto_implementation_request(id: i64, params: TextDocumentPositionParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/implementation",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 prepareCallHierarchy 请求
pub fn prepare_call_hierarchy_request(
    id: i64,
    params: CallHierarchyPrepareParams,
) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "textDocument/prepareCallHierarchy",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 incomingCalls 请求
pub fn incoming_calls_request(id: i64, params: CallHierarchyIncomingCallsParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "callHierarchy/incomingCalls",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构建 outgoingCalls 请求
pub fn outgoing_calls_request(id: i64, params: CallHierarchyOutgoingCallsParams) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "callHierarchy/outgoingCalls",
        Some(serde_json::to_value(params).unwrap_or(Value::Null)),
    )
}

/// 构造 initialize 请求参数
pub fn initialize_params(
    root_uri: String,
    workspace_folders: Vec<WorkspaceFolder>,
    initialization_options: Option<Value>,
) -> Value {
    let mut params = serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "workspaceFolders": workspace_folders,
        "capabilities": {
            "textDocument": {
                "definition": { "dynamicRegistration": false },
                "references": { "dynamicRegistration": false },
                "hover": { "dynamicRegistration": false, "contentFormat": ["plaintext", "markdown"] },
                "documentSymbol": { "dynamicRegistration": false, "hierarchicalDocumentSymbolSupport": true },
                "implementation": { "dynamicRegistration": false },
                "typeHierarchy": { "dynamicRegistration": false },
                "callHierarchy": { "dynamicRegistration": false },
                "publishDiagnostics": { "relatedInformation": true }
            },
            "workspace": {
                "symbol": { "dynamicRegistration": false }
            }
        }
    });
    if let Some(opts) = initialization_options {
        params["initializationOptions"] = opts;
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_workspace_folders() -> Vec<WorkspaceFolder> {
        vec![WorkspaceFolder {
            uri: "file:///tmp".parse().unwrap(),
            name: "workspace".to_string(),
        }]
    }

    #[test]
    fn test_initialize_params_without_init_options() {
        let params = initialize_params("file:///tmp".to_string(), make_workspace_folders(), None);
        assert_eq!(params["rootUri"], "file:///tmp");
        assert!(params.get("initializationOptions").is_none());
        assert!(params["capabilities"]["textDocument"]["definition"].is_object());
    }

    #[test]
    fn test_initialize_params_with_init_options() {
        let opts = serde_json::json!({
            "maxTsServerMemory": 8192,
            "checkOnSave": { "command": "clippy" }
        });
        let params = initialize_params(
            "file:///tmp".to_string(),
            make_workspace_folders(),
            Some(opts),
        );
        assert_eq!(params["initializationOptions"]["maxTsServerMemory"], 8192);
        assert_eq!(
            params["initializationOptions"]["checkOnSave"]["command"],
            "clippy"
        );
    }

    #[test]
    fn test_initialize_params_has_process_id() {
        let params = initialize_params("file:///tmp".to_string(), make_workspace_folders(), None);
        assert!(params["processId"].is_number());
        assert!(params["processId"].as_u64().unwrap() > 0);
    }
}
