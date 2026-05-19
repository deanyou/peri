//! ACP Stdio 模式：通过 stdin/stdout JSON-RPC 与 IDE client 通信

use std::sync::Arc;

// ─── ACP Stdio 类型 ──────────────────────────────────────────────────────

struct SessionInfo {
    #[allow(dead_code)]
    session_id: String,
    thread_id: String,
    cwd: String,
    history: Vec<peri_agent::messages::BaseMessage>,
    cancel_token: Option<peri_agent::agent::AgentCancellationToken>,
}

struct StdioContext {
    provider: parking_lot::RwLock<peri_tui::app::agent::LlmProvider>,
    peri_config: parking_lot::RwLock<peri_tui::config::PeriConfig>,
    permission_mode: Arc<peri_middlewares::prelude::SharedPermissionMode>,
    cron_scheduler: Arc<parking_lot::Mutex<peri_middlewares::cron::CronScheduler>>,
    mcp_pool: Option<Arc<peri_middlewares::mcp::McpClientPool>>,
    plugin_skill_dirs: Vec<std::path::PathBuf>,
    plugin_agent_dirs: Vec<std::path::PathBuf>,
    hook_groups: Vec<Vec<peri_middlewares::hooks::RegisteredHook>>,
    plugin_lsp_servers: Vec<peri_lsp::config::LspServerConfig>,
    tool_search_index: Arc<peri_middlewares::tool_search::ToolSearchIndex>,
    shared_tools: Arc<
        parking_lot::RwLock<
            std::collections::HashMap<String, Arc<dyn peri_agent::tools::BaseTool>>,
        >,
    >,
    sessions: parking_lot::RwLock<std::collections::HashMap<String, SessionInfo>>,
    thread_store: Arc<dyn peri_agent::thread::ThreadStore>,
}

/// Stdio 模式下的简化 Broker：直接 approve 所有权限请求，questions 返回空答案。
struct StdioBroker;

impl StdioBroker {
    fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl peri_agent::interaction::UserInteractionBroker for StdioBroker {
    async fn request(
        &self,
        context: peri_agent::interaction::InteractionContext,
    ) -> peri_agent::interaction::InteractionResponse {
        match context {
            peri_agent::interaction::InteractionContext::Approval { items } => {
                peri_agent::interaction::InteractionResponse::Decisions(
                    items
                        .into_iter()
                        .map(|_| peri_agent::interaction::ApprovalDecision::Approve)
                        .collect(),
                )
            }
            peri_agent::interaction::InteractionContext::Questions { requests } => {
                peri_agent::interaction::InteractionResponse::Answers(
                    requests
                        .into_iter()
                        .map(|q| peri_agent::interaction::QuestionAnswer {
                            id: q.id,
                            selected: vec![],
                            text: Some(String::new()),
                        })
                        .collect(),
                )
            }
        }
    }
}

// ─── run_acp_stdio ───────────────────────────────────────────────────────

pub async fn run_acp_stdio(cwd: String) -> anyhow::Result<()> {
    let _telemetry = peri_agent::telemetry::init_tracing("peri-acp");

    // 解析工作目录
    let cwd = std::path::Path::new(&cwd)
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(&cwd))
        .to_string_lossy()
        .to_string();

    // 加载配置
    let peri_config = peri_tui::config::load().unwrap_or_default();
    let provider = peri_tui::app::agent::LlmProvider::from_config(&peri_config)
        .or_else(peri_tui::app::agent::LlmProvider::from_env)
        .ok_or_else(|| anyhow::anyhow!("No LLM provider configured. Set ANTHROPIC_API_KEY or OPENAI_API_KEY, or configure ~/.peri/settings.json"))?;

    tracing::info!(
        provider = %provider.display_name(),
        model = %provider.model_name(),
        cwd = %cwd,
        "ACP stdio mode starting"
    );

    // 初始化 cron scheduler
    let cron_scheduler = {
        let scheduler =
            peri_middlewares::cron::CronScheduler::new(tokio::sync::mpsc::unbounded_channel().0);
        Arc::new(parking_lot::Mutex::new(scheduler))
    };

    // 初始化 MCP 连接池（后台）
    let mcp_pool = {
        use peri_middlewares::mcp::{McpClientPool, McpInitStatus};
        let pool = Arc::new(McpClientPool::new_pending());
        let pool_clone = pool.clone();
        let (init_tx, _init_rx) = tokio::sync::watch::channel(McpInitStatus::Pending);
        let cwd_clone = cwd.clone();
        let claude_home = dirs_next::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".claude");
        tokio::spawn(async move {
            McpClientPool::run_initialize(
                pool_clone,
                std::path::Path::new(&cwd_clone),
                &claude_home,
                init_tx,
                None,
            )
            .await;
        });
        Some(pool)
    };

    // 加载插件数据
    let claude_dir = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".claude");
    let plugin_data = peri_middlewares::plugin::load_enabled_plugins_aggregated(&claude_dir);

    let plugin_skill_dirs = plugin_data.all_skill_dirs.clone();
    let plugin_agent_dirs = plugin_data.all_agent_dirs.clone();
    let plugin_lsp_servers = plugin_data.all_lsp_servers.clone();
    let plugin_hooks = plugin_data.all_hooks.clone();

    // 组装 hook groups
    let mut hook_groups: Vec<Vec<peri_middlewares::hooks::RegisteredHook>> = Vec::new();
    if !plugin_hooks.is_empty() {
        hook_groups.push(plugin_hooks);
    }
    let local_hooks = peri_middlewares::hooks::loader::load_settings_local_hooks(&cwd);
    if !local_hooks.is_empty() {
        hook_groups.push(local_hooks);
    }

    let permission_mode = peri_middlewares::prelude::SharedPermissionMode::new(
        peri_middlewares::prelude::PermissionMode::AutoMode,
    );
    let tool_search_index = Arc::new(peri_middlewares::tool_search::ToolSearchIndex::new());
    let shared_tools = Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new()));

    // 初始化 thread 存储（失败时 fallback 到临时目录）
    let thread_store: Arc<dyn peri_agent::thread::ThreadStore> =
        match peri_tui::thread::SqliteThreadStore::default_path().await {
            Ok(store) => Arc::new(store),
            Err(_) => Arc::new(
                peri_tui::thread::SqliteThreadStore::new(
                    std::env::temp_dir().join("zen-threads.db"),
                )
                .await
                .expect("无法创建临时 SQLite 数据库"),
            ),
        };

    // 构建共享的 ServerContext，所有请求处理器通过 Arc 共享
    let ctx = Arc::new(StdioContext {
        provider: parking_lot::RwLock::new(provider),
        peri_config: parking_lot::RwLock::new(peri_config),
        permission_mode,
        cron_scheduler,
        mcp_pool,
        plugin_skill_dirs,
        plugin_agent_dirs,
        hook_groups,
        plugin_lsp_servers,
        tool_search_index,
        shared_tools,
        sessions: parking_lot::RwLock::new(std::collections::HashMap::new()),
        thread_store,
    });

    use agent_client_protocol::schema::{
        AgentCapabilities, CancelNotification, InitializeRequest, InitializeResponse,
        NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, ProtocolVersion,
        SessionId, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
        SetSessionModeRequest, SetSessionModeResponse, SetSessionModelRequest,
        SetSessionModelResponse, StopReason,
    };
    use agent_client_protocol::{Agent, Client, ConnectionTo};
    use agent_client_protocol_tokio::Stdio;
    use peri_acp::session::event_sink::StdioEventSink;
    use peri_acp::session::executor;
    use peri_acp::session::state_builders::{
        apply_thinking_effort, build_config_options, build_mode_state, build_model_state,
        parse_permission_mode,
    };
    use peri_agent::agent::AgentCancellationToken;

    let ctx_clone = ctx.clone();

    Agent
        .builder()
        .name("peri-acp")
        // ── initialize ──
        .on_receive_request(
            async move |_req: InitializeRequest, responder, _cx| {
                tracing::info!("ACP initialize");
                responder.respond(
                    InitializeResponse::new(ProtocolVersion::V1)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/new ──
        .on_receive_request(
            {
                let ctx = ctx_clone.clone();
                async move |req: NewSessionRequest, responder, _cx| {
                    let cwd_str = req.cwd.to_string_lossy().to_string();
                    let meta = peri_agent::thread::ThreadMeta::new(&cwd_str);
                    let thread_id = match ctx.thread_store.create_thread(meta).await {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::error!(error = %e, "Thread creation failed");
                            let _ = responder.respond(NewSessionResponse::new(SessionId::new("error")));
                            return Ok(());
                        }
                    };
                    let sid = thread_id.clone();
                    {
                        let mut sessions = ctx.sessions.write();
                        sessions.insert(
                            sid.clone(),
                            SessionInfo {
                                session_id: sid.clone(),
                                thread_id: thread_id.clone(),
                                cwd: cwd_str,
                                history: Vec::new(),
                                cancel_token: None,
                            },
                        );
                    }
                    tracing::info!(session_id = %sid, "ACP session created with ThreadStore");
                    let modes = build_mode_state(&ctx.permission_mode);
                    let models = {
                        let p = ctx.provider.read();
                        let c = ctx.peri_config.read();
                        build_model_state(&p, &c)
                    };
                    let config_options = {
                        let c = ctx.peri_config.read();
                        let p = ctx.provider.read();
                        build_config_options(&c, &p, ctx.permission_mode.load())
                    };
                    responder.respond(
                        NewSessionResponse::new(SessionId::new(&*sid))
                            .modes(modes)
                            .models(models)
                            .config_options(config_options),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/prompt ──
        .on_receive_request(
            {
                let ctx = ctx_clone.clone();
                async move |req: PromptRequest, responder, cx: ConnectionTo<Client>| {
                    let sid = req.session_id.0.to_string();
                    let content: String = req.prompt.iter().filter_map(|b| {
                        if let agent_client_protocol::schema::ContentBlock::Text(t) = b {
                            Some(t.text.as_str())
                        } else {
                            None
                        }
                    }).collect::<Vec<&str>>().join("");

                    let (agent_cwd, history, is_empty_history, thread_id) = {
                        let sessions = ctx.sessions.read();
                        match sessions.get(&sid) {
                            Some(s) => (s.cwd.clone(), s.history.clone(), s.history.is_empty(), s.thread_id.clone()),
                            None => {
                                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                                return Ok(());
                            }
                        }
                    };
                    let history_len = history.len();

                    let cancel = AgentCancellationToken::new();
                    {
                        let mut sessions = ctx.sessions.write();
                        if let Some(s) = sessions.get_mut(&sid) {
                            s.cancel_token = Some(cancel.clone());
                        }
                    }

                    let broker: Arc<dyn peri_agent::interaction::UserInteractionBroker> =
                        Arc::new(StdioBroker::new());

                    let event_sink = Arc::new(StdioEventSink::new(cx, req.session_id.clone()));
                    let provider_snapshot = ctx.provider.read().clone();
                    let peri_config_snapshot = Arc::new(ctx.peri_config.read().clone());

                    let result = executor::execute_prompt(
                        &provider_snapshot,
                        peri_config_snapshot,
                        &agent_cwd,
                        content,
                        history,
                        is_empty_history,
                        ctx.permission_mode.clone(),
                        event_sink,
                        cancel,
                        broker,
                        ctx.plugin_skill_dirs.clone(),
                        ctx.plugin_agent_dirs.clone(),
                        ctx.hook_groups.clone(),
                        Some(ctx.cron_scheduler.clone()),
                        sid.clone(),
                        ctx.mcp_pool.clone(),
                        ctx.tool_search_index.clone(),
                        ctx.shared_tools.clone(),
                        ctx.plugin_lsp_servers.clone(),
                    )
                    .await;

                    // Persist new messages to ThreadStore and update in-memory state.
                    if result.ok && history_len < result.messages.len() {
                        let new_msgs = &result.messages[history_len..];
                        if let Err(e) = ctx.thread_store.append_messages(&thread_id, new_msgs).await {
                            tracing::warn!(error = %e, "Failed to persist messages to ThreadStore");
                        }
                    }
                    {
                        let mut sessions = ctx.sessions.write();
                        if let Some(s) = sessions.get_mut(&sid) {
                            s.history = result.messages;
                            s.cancel_token = None;
                        }
                    }

                    let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/set_mode ──
        .on_receive_request(
            {
                let ctx = ctx_clone.clone();
                async move |req: SetSessionModeRequest, responder, _cx| {
                    let mode_id = req.mode_id.0.as_ref();
                    let mode = parse_permission_mode(mode_id);
                    ctx.permission_mode.store(mode);
                    tracing::info!(mode_id = %mode_id, "Permission mode changed");
                    responder.respond(SetSessionModeResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/set_model ──
        .on_receive_request(
            {
                let ctx = ctx_clone.clone();
                async move |req: SetSessionModelRequest, responder, _cx| {
                    let model_id = req.model_id.0.to_string();
                    let new_provider = {
                        let cfg = ctx.peri_config.read();
                        peri_tui::app::agent::LlmProvider::from_config_for_alias(&cfg, &model_id)
                    };
                    if let Some(new_provider) = new_provider {
                        tracing::info!(model_id = %model_id, model = %new_provider.model_name(), "Model changed");
                        *ctx.provider.write() = new_provider;
                    }
                    responder.respond(SetSessionModelResponse::new())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/set_config_option ──
        .on_receive_request(
            {
                let ctx = ctx_clone.clone();
                async move |req: SetSessionConfigOptionRequest, responder, _cx| {
                    let config_id = req.config_id.0.as_ref();
                    match &req.value {
                        agent_client_protocol_schema::SessionConfigOptionValue::ValueId { value } => {
                            let v = value.0.as_ref();
                            match config_id {
                                "mode" => {
                                    let mode = parse_permission_mode(v);
                                    ctx.permission_mode.store(mode);
                                    tracing::info!(mode = %v, "Permission mode changed via configOption");
                                }
                                "model" => {
                                    let new_provider = {
                                        let cfg = ctx.peri_config.read();
                                        peri_tui::app::agent::LlmProvider::from_config_for_alias(&cfg, v)
                                    };
                                    if let Some(new_provider) = new_provider {
                                        tracing::info!(model_id = %v, model = %new_provider.model_name(), "Model changed via configOption");
                                        *ctx.provider.write() = new_provider;
                                    }
                                }
                                "thinking_effort" => {
                                    apply_thinking_effort(&ctx.peri_config, v);
                                    tracing::info!(effort = %v, "Thinking effort changed via configOption");
                                }
                                _ => {
                                    tracing::debug!(config_id = %config_id, "Unknown config option");
                                }
                            }
                        }
                        agent_client_protocol_schema::SessionConfigOptionValue::Boolean { value: _ } => {
                            tracing::debug!(config_id = %config_id, "Boolean config option not handled");
                        }
                        _ => {
                            tracing::debug!(config_id = %config_id, "Unknown config option value type");
                        }
                    }
                    let config_options = {
                        let cfg = ctx.peri_config.read();
                        let p = ctx.provider.read();
                        build_config_options(&cfg, &p, ctx.permission_mode.load())
                    };
                    responder.respond(SetSessionConfigOptionResponse::new(config_options))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // ── session/cancel ──
        .on_receive_notification(
            {
                let ctx = ctx_clone.clone();
                async move |_notif: CancelNotification, _cx| {
                    let sid: &str = &_notif.session_id.0;
                    let sessions = ctx.sessions.read();
                    if let Some(s) = sessions.get(sid) {
                        if let Some(ref token) = s.cancel_token {
                            token.cancel();
                            tracing::info!(session_id = %sid, "Cancel requested");
                        }
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("ACP error: {e}"))
}
