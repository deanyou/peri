//! Shared prompt execution logic.
//!
//! Provides [`execute_prompt`] which encapsulates the common agent execution
//! pipeline used by both TUI (via [`TransportEventSink`]) and stdio (via
//! [`StdioEventSink`]) paths.

use std::sync::Arc;

use peri_agent::agent::events::{AgentEvent as ExecutorEvent, AgentEventHandler};
use peri_agent::agent::state::AgentState;
use peri_agent::agent::AgentCancellationToken;
use peri_agent::interaction::UserInteractionBroker;
use peri_agent::messages::BaseMessage;
use tokio::sync::oneshot;
use tracing::{debug, error};

use crate::agent::builder::{self, AcpAgentConfig};
use crate::prompt::{build_system_prompt, PromptFeatures};
use crate::provider::LlmProvider;
use crate::session::event_sink::EventSink;

/// Result of prompt execution.
pub struct PromptResult {
    /// Updated message history after execution.
    pub messages: Vec<BaseMessage>,
    /// Whether execution succeeded.
    pub ok: bool,
}

/// Shared agent execution pipeline.
///
/// This function encapsulates steps 2-7 of the prompt execution flow:
/// 1. Create event channel + cancel token
/// 2. Build agent via [`build_system_prompt`] + [`builder::build_agent`]
/// 3. Spawn background event pump using the provided [`EventSink`]
/// 4. Execute agent
/// 5. Wait for pump to drain
/// 6. Return updated messages
///
/// The caller is responsible for:
/// - Session management (storing/retrieving cwd, history, cancel_token)
/// - Choosing the broker (HITL/AskUser handler)
/// - Providing the correct `EventSink` implementation
#[allow(clippy::too_many_arguments)]
pub async fn execute_prompt(
    provider: &LlmProvider,
    peri_config: Arc<crate::provider::PeriConfig>,
    cwd: &str,
    content: String,
    history: Vec<BaseMessage>,
    is_empty_history: bool,
    permission_mode: Arc<peri_middlewares::prelude::SharedPermissionMode>,
    event_sink: Arc<dyn EventSink>,
    cancel: AgentCancellationToken,
    broker: Arc<dyn UserInteractionBroker>,
    plugin_skill_dirs: Vec<std::path::PathBuf>,
    plugin_agent_dirs: Vec<std::path::PathBuf>,
    hook_groups: Vec<Vec<peri_middlewares::hooks::RegisteredHook>>,
    cron_scheduler: Option<Arc<parking_lot::Mutex<peri_middlewares::cron::CronScheduler>>>,
    session_id: String,
    mcp_pool: Option<Arc<peri_middlewares::mcp::McpClientPool>>,
    tool_search_index: Arc<peri_middlewares::tool_search::ToolSearchIndex>,
    shared_tools: Arc<
        parking_lot::RwLock<
            std::collections::HashMap<String, Arc<dyn peri_agent::tools::BaseTool>>,
        >,
    >,
    lsp_servers: Vec<peri_lsp::config::LspServerConfig>,
) -> PromptResult {
    let agent_input = peri_agent::agent::react::AgentInput::text(content);

    // Event channel
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutorEvent>();
    let event_tx = Arc::new(std::sync::Mutex::new(Some(event_tx)));

    let event_handler: Arc<dyn AgentEventHandler> =
        Arc::new(peri_agent::agent::events::FnEventHandler({
            let event_tx = event_tx.clone();
            move |event: ExecutorEvent| {
                if let Some(tx) = event_tx.lock().unwrap().as_ref() {
                    let _ = tx.send(event);
                }
            }
        }));

    let features = PromptFeatures::detect();
    let system_prompt = build_system_prompt(None, cwd, features, &plugin_agent_dirs);
    let context_window = provider.context_window();

    let agent_output = builder::build_agent(AcpAgentConfig {
        provider: provider.clone(),
        cwd: cwd.to_string(),
        system_prompt,
        event_handler,
        cancel: cancel.clone(),
        permission_mode,
        peri_config,
        cron_scheduler,
        agent_overrides: None,
        preload_skills: Vec::new(),
        session_id: Some(session_id.clone()),
        broker,
        plugin_skill_dirs,
        plugin_agent_dirs,
        hook_groups,
        hook_session_start: is_empty_history,
        mcp_pool,
        tool_search_index,
        shared_tools,
        child_handler_factory: None,
        lsp_servers,
    });

    // Background event pump
    let sink = event_sink;
    let sid = session_id.clone();
    let (pump_done_tx, pump_done_rx) = oneshot::channel();
    tokio::spawn(async move {
        while let Some(exec_event) = event_rx.recv().await {
            sink.push_event(&sid, &exec_event, context_window).await;
        }
        sink.push_done(&sid).await;
        let _ = pump_done_tx.send(());
    });

    // Execute agent
    let mut agent_state = AgentState::with_messages(cwd.to_string(), history);
    let result = agent_output
        .executor
        .execute(agent_input, &mut agent_state, Some(cancel))
        .await;
    drop(agent_output);

    // Close event channel and wait for pump
    {
        let mut tx_guard = event_tx.lock().unwrap();
        *tx_guard = None;
    }
    match pump_done_rx.await {
        Ok(()) => debug!(session_id = %session_id, "Event pump done"),
        Err(_) => error!(session_id = %session_id, "Event pump done channel closed unexpectedly"),
    }

    let ok = result.is_ok();
    if let Err(e) = &result {
        error!(session_id = %session_id, error = %e, "Agent execution failed");
    }

    PromptResult {
        messages: agent_state.into_messages(),
        ok,
    }
}
