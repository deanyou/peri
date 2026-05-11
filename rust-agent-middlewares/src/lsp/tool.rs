use async_trait::async_trait;
use perihelion_lsp::pool::LspServerPool;
use rust_create_agent::tools::BaseTool;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

use super::formatters;

#[derive(Debug, Error)]
enum LspToolError {
    #[error("缺少参数: {0}")]
    MissingParam(String),
    #[error("无效的 operation: {0}")]
    InvalidOperation(String),
    #[error("没有为该文件找到 LSP 服务器: {0}")]
    NoServer(String),
    #[error("LSP 请求失败: {0}")]
    RequestFailed(String),
    #[error("LSP 服务器未就绪")]
    NotReady,
    #[error("行号和列号必须 >= 1")]
    InvalidPosition,
}

const TOOL_NAME: &str = "LSP";

const DESCRIPTION: &str = "\
Provides code intelligence via Language Server Protocol (LSP). Use this tool for navigating and understanding code — \
go to definitions, find references, get type information, view document symbols, and check diagnostics.\
\n\nOperations:\n\
- goToDefinition: Find where a symbol is defined\n\
- findReferences: Find all usages of a symbol\n\
- hover: Get type/documentation info at a position\n\
- documentSymbol: List all symbols in a file\n\
- workspaceSymbol: Search for symbols across the workspace\n\
- goToImplementation: Find implementations of an interface/abstract\n\
- prepareCallHierarchy: Get call hierarchy item at a position\n\
- incomingCalls: Find all callers of a function\n\
- outgoingCalls: Find all functions called by a function\n\
- diagnostics: Get diagnostic errors/warnings for a file";

fn parameters_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "operation": {
                "type": "string",
                "description": "The LSP operation to perform",
                "enum": [
                    "goToDefinition",
                    "findReferences",
                    "hover",
                    "documentSymbol",
                    "workspaceSymbol",
                    "goToImplementation",
                    "prepareCallHierarchy",
                    "incomingCalls",
                    "outgoingCalls",
                    "diagnostics"
                ]
            },
            "file_path": {
                "type": "string",
                "description": "Absolute or relative path to the file"
            },
            "line": {
                "type": "integer",
                "description": "Line number (1-based, as shown in editors)"
            },
            "character": {
                "type": "integer",
                "description": "Character/column offset (1-based, as shown in editors)"
            },
            "query": {
                "type": "string",
                "description": "Search query (for workspaceSymbol operation)"
            }
        },
        "required": ["operation"]
    })
}

pub struct LspTool {
    pool: Arc<LspServerPool>,
}

impl LspTool {
    pub fn new(pool: Arc<LspServerPool>) -> Self {
        Self { pool }
    }

    fn file_to_uri(file_path: &str) -> String {
        if file_path.starts_with("file://") {
            file_path.to_string()
        } else {
            format!("file://{}", file_path)
        }
    }

    /// 获取已就绪的服务器，必要时初始化
    async fn get_initialized_server(
        &self,
        file_path: &str,
    ) -> Result<Arc<perihelion_lsp::client::LspClient>, LspToolError> {
        // 同步检查：guard 在表达式结束时释放
        let needs_init = match self.pool.server_for_file(file_path) {
            Some(server) if server.is_ready() => return Ok(server),
            _ => true,
        };

        if needs_init {
            self.pool
                .ensure_initialized()
                .await
                .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
        }

        self.pool
            .server_for_file(file_path)
            .filter(|s| s.is_ready())
            .ok_or(LspToolError::NotReady)
    }

    /// 获取任意一个已就绪的服务器
    async fn get_any_ready_server(
        &self,
    ) -> Result<Arc<perihelion_lsp::client::LspClient>, LspToolError> {
        if let Some(s) = self.pool.any_server() {
            return Ok(s);
        }

        self.pool
            .ensure_initialized()
            .await
            .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;

        self.pool.any_server().ok_or(LspToolError::NotReady)
    }
}

#[async_trait]
impl BaseTool for LspTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> Value {
        parameters_schema()
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LspToolError::MissingParam("operation".to_string()))?;

        // diagnostics 操作
        if operation == "diagnostics" {
            let file_path = input.get("file_path").and_then(|v| v.as_str());
            let entries = if let Some(fp) = file_path {
                let uri = Self::file_to_uri(fp);
                self.pool.diagnostics().get_for_file(&uri)
            } else {
                self.pool.diagnostics().get_all()
            };
            return Ok(formatters::format_diagnostics(&entries));
        }

        // workspaceSymbol 操作
        if operation == "workspaceSymbol" {
            let query = input
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LspToolError::MissingParam("query".to_string()))?;
            let server = self.get_any_ready_server().await?;
            let result = server
                .request(
                    "workspace/symbol",
                    Some(serde_json::json!({ "query": query })),
                    10_000,
                )
                .await
                .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
            return Ok(formatters::format_workspace_symbols(&result));
        }

        // documentSymbol 只需要 file_path
        if operation == "documentSymbol" {
            let file_path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LspToolError::MissingParam("file_path".to_string()))?;
            let server = self.get_initialized_server(file_path).await?;
            let uri = Self::file_to_uri(file_path);
            let params = serde_json::json!({
                "textDocument": { "uri": uri }
            });
            let result = server
                .request("textDocument/documentSymbol", Some(params), 10_000)
                .await
                .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
            return Ok(formatters::format_document_symbols(&result));
        }

        // 以下操作需要 file_path, line, character
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LspToolError::MissingParam("file_path".to_string()))?;
        let line = input
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| LspToolError::MissingParam("line".to_string()))?;
        let character = input
            .get("character")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .ok_or_else(|| LspToolError::MissingParam("character".to_string()))?;

        if line == 0 || character == 0 {
            return Err(Box::new(LspToolError::InvalidPosition));
        }

        let server = self.get_initialized_server(file_path).await?;

        let uri = Self::file_to_uri(file_path);
        let lsp_line = line - 1;
        let lsp_char = character - 1;
        let text_document_position = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": lsp_line, "character": lsp_char }
        });

        let timeout = 10_000u64;

        match operation {
            "goToDefinition" => {
                let result = server
                    .request(
                        "textDocument/definition",
                        Some(text_document_position),
                        timeout,
                    )
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_definition_result(&result))
            }

            "findReferences" => {
                let params = serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                    "context": { "includeDeclaration": true }
                });
                let result = server
                    .request("textDocument/references", Some(params), timeout)
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_references(&result))
            }

            "hover" => {
                let result = server
                    .request("textDocument/hover", Some(text_document_position), timeout)
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_hover(&result))
            }

            "documentSymbol" => unreachable!("handled above"),

            "goToImplementation" => {
                let result = server
                    .request(
                        "textDocument/implementation",
                        Some(text_document_position),
                        timeout,
                    )
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_definition_result(&result))
            }

            "prepareCallHierarchy" => {
                let result = server
                    .request(
                        "textDocument/prepareCallHierarchy",
                        Some(text_document_position),
                        timeout,
                    )
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_call_hierarchy_items(&result))
            }

            "incomingCalls" => {
                let prepare_result = server
                    .request(
                        "textDocument/prepareCallHierarchy",
                        Some(text_document_position),
                        timeout,
                    )
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;

                let items: Vec<lsp_types::CallHierarchyItem> =
                    serde_json::from_value(prepare_result)
                        .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;

                if items.is_empty() {
                    return Ok("No call hierarchy item found at this position.".to_string());
                }

                let params = serde_json::json!({ "item": items[0] });
                let result = server
                    .request("callHierarchy/incomingCalls", Some(params), timeout)
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                let calls: Vec<lsp_types::CallHierarchyIncomingCall> =
                    serde_json::from_value(result)
                        .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_incoming_calls(&calls))
            }

            "outgoingCalls" => {
                let prepare_result = server
                    .request(
                        "textDocument/prepareCallHierarchy",
                        Some(text_document_position),
                        timeout,
                    )
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;

                let items: Vec<lsp_types::CallHierarchyItem> =
                    serde_json::from_value(prepare_result)
                        .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;

                if items.is_empty() {
                    return Ok("No call hierarchy item found at this position.".to_string());
                }

                let params = serde_json::json!({ "item": items[0] });
                let result = server
                    .request("callHierarchy/outgoingCalls", Some(params), timeout)
                    .await
                    .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                let calls: Vec<lsp_types::CallHierarchyOutgoingCall> =
                    serde_json::from_value(result)
                        .map_err(|e| LspToolError::RequestFailed(e.to_string()))?;
                Ok(formatters::format_outgoing_calls(&calls))
            }

            _ => Err(Box::new(LspToolError::InvalidOperation(
                operation.to_string(),
            ))),
        }
    }
}
