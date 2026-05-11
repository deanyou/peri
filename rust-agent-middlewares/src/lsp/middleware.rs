use std::sync::Arc;

use async_trait::async_trait;
use perihelion_lsp::config::{LspConfigFile, LspServerConfig};
use perihelion_lsp::pool::LspServerPool;
use rust_create_agent::agent::react::{ToolCall, ToolResult};
use rust_create_agent::agent::state::State;
use rust_create_agent::error::AgentResult;
use rust_create_agent::middleware::Middleware;
use rust_create_agent::tools::BaseTool;

use super::tool::LspTool;

pub struct LspMiddleware {
    pool: Arc<LspServerPool>,
}

impl LspMiddleware {
    pub fn new(root_uri: String, config: LspConfigFile) -> Self {
        let pool = Arc::new(LspServerPool::new(&root_uri, config));
        Self { pool }
    }

    pub fn from_configs(root_uri: String, configs: Vec<LspServerConfig>) -> Self {
        let config = LspConfigFile {
            lsp_servers: configs.into_iter().map(|c| (c.name.clone(), c)).collect(),
        };
        Self::new(root_uri, config)
    }

    pub fn shared_pool(&self) -> Arc<LspServerPool> {
        Arc::clone(&self.pool)
    }
}

#[async_trait]
impl<S: State> Middleware<S> for LspMiddleware {
    fn name(&self) -> &str {
        "LspMiddleware"
    }

    fn collect_tools(&self, _cwd: &str) -> Vec<Box<dyn BaseTool>> {
        if !self.pool.has_servers() {
            return Vec::new();
        }
        vec![Box::new(LspTool::new(Arc::clone(&self.pool)))]
    }

    async fn after_tool(
        &self,
        _state: &mut S,
        tool_call: &ToolCall,
        _result: &ToolResult,
    ) -> AgentResult<()> {
        if tool_call.name != "Write" && tool_call.name != "Edit" {
            return Ok(());
        }

        let file_path = match tool_call.input.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => return Ok(()),
        };

        let server = match self.pool.server_for_file(&file_path) {
            Some(s) if s.is_ready() => s,
            _ => return Ok(()),
        };

        let uri = format!("file://{}", file_path);
        let text = match tokio::fs::read_to_string(&file_path).await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(target: "lsp", file = %file_path, error = %e, "LSP 同步文件时读取失败");
                return Ok(());
            }
        };

        if let Err(e) = server.did_change(&uri, &text).await {
            tracing::debug!(target: "lsp", file = %file_path, error = %e, "LSP didChange 失败");
        }
        if let Err(e) = server.did_save(&uri).await {
            tracing::debug!(target: "lsp", file = %file_path, error = %e, "LSP didSave 失败");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perihelion_lsp::config::LspServerConfig;
    use rust_create_agent::agent::state::AgentState;
    use std::collections::HashMap;

    fn make_config(name: &str, exts: Vec<(&str, &str)>) -> LspServerConfig {
        LspServerConfig {
            name: name.to_string(),
            command: name.to_string(),
            args: vec!["--stdio".to_string()],
            env: None,
            extension_to_language: exts
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            initialization_options: None,
            disabled: None,
            max_restarts: None,
            startup_timeout: None,
            source: None,
        }
    }

    #[test]
    fn test_middleware_name() {
        let config = LspConfigFile {
            lsp_servers: HashMap::new(),
        };
        let mw = LspMiddleware::new("/tmp".to_string(), config);
        assert_eq!(
            <LspMiddleware as Middleware<AgentState>>::name(&mw),
            "LspMiddleware"
        );
    }

    #[test]
    fn test_collect_tools_empty_config() {
        let config = LspConfigFile {
            lsp_servers: HashMap::new(),
        };
        let mw = LspMiddleware::new("/tmp".to_string(), config);
        let tools = <LspMiddleware as Middleware<AgentState>>::collect_tools(&mw, "/tmp");
        assert!(tools.is_empty());
    }

    #[test]
    fn test_collect_tools_with_servers() {
        let mut servers = HashMap::new();
        servers.insert(
            "rust-analyzer".to_string(),
            make_config("rust-analyzer", vec![(".rs", "rust")]),
        );
        let config = LspConfigFile {
            lsp_servers: servers,
        };
        let mw = LspMiddleware::new("/tmp".to_string(), config);
        let tools = <LspMiddleware as Middleware<AgentState>>::collect_tools(&mw, "/tmp");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "LSP");
    }

    #[test]
    fn test_shared_pool() {
        let mut servers = HashMap::new();
        servers.insert(
            "rust-analyzer".to_string(),
            make_config("rust-analyzer", vec![(".rs", "rust")]),
        );
        let config = LspConfigFile {
            lsp_servers: servers,
        };
        let mw = LspMiddleware::new("/tmp".to_string(), config);
        let pool = mw.shared_pool();
        assert!(pool.has_servers());
    }

    #[test]
    fn test_from_configs() {
        let configs = vec![make_config("rust-analyzer", vec![(".rs", "rust")])];
        let mw = LspMiddleware::from_configs("/tmp".to_string(), configs);
        assert_eq!(
            <LspMiddleware as Middleware<AgentState>>::name(&mw),
            "LspMiddleware"
        );
        assert!(mw.pool.has_servers());
    }
}
