use super::message_pipeline::PipelineAction;
use super::*;

/// 从输入文本中提取 `/skill-name` 格式的 token（字母、数字、连字符、下划线）
fn extract_skill_tokens(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter(|token| token.starts_with('/') && token.len() > 1)
        .map(|token| {
            let name = token.trim_start_matches('/');
            name.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

impl App {
    pub fn submit_message(&mut self, input: String) {
        if input.trim().is_empty() {
            return;
        }

        // 记录提交前的状态长度，用于中断时回滚 agent_state_messages
        self.session_mgr.sessions[self.session_mgr.active]
            .metadata
            .pre_submit_state_len = self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_state_messages
            .len();

        self.push_input_history(input.clone());

        // 消费待发送附件
        let attachments = std::mem::take(
            &mut self.session_mgr.sessions[self.session_mgr.active]
                .metadata
                .pending_attachments,
        );

        // 构建用于显示的文字（附件摘要追加在末尾）
        let display = if attachments.is_empty() {
            input.clone()
        } else {
            format!("{} [🖼 {} 张图片]", input, attachments.len())
        };
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .pipeline
            .begin_round();
        let user_vm = MessageViewModel::user(display.clone());
        self.apply_pipeline_action(PipelineAction::AddMessage(user_vm));
        // round_start_vm_idx 在 UserBubble 推入之后设置，
        // 确保 RebuildAll 不会截掉当前轮次的用户消息
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .round_start_vm_idx = self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .view_messages
            .len();
        self.session_mgr.sessions[self.session_mgr.active]
            .metadata
            .last_human_message = Some(display);
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .last_submitted_text = Some(input.clone());
        self.set_loading(true);
        self.session_mgr.sessions[self.session_mgr.active]
            .ui
            .scroll_offset = u16::MAX;
        self.session_mgr.sessions[self.session_mgr.active]
            .ui
            .scroll_follow = true;
        self.session_mgr.sessions[self.session_mgr.active]
            .todo_items
            .clear();

        // 开始计时新任务
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .task_start_time = Some(std::time::Instant::now());
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .last_task_duration = None;
        if self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .session_start_time
            .is_none()
        {
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .session_start_time = Some(std::time::Instant::now());
        }

        let provider = match self
            .services
            .peri_config
            .as_ref()
            .and_then(agent::LlmProvider::from_config)
            .or_else(agent::LlmProvider::from_env)
        {
            Some(p) => p,
            None => {
                self.apply_pipeline_action(PipelineAction::AddMessage(MessageViewModel::system(
                    "未配置 API Key，请输入 /login 配置 Provider".to_string(),
                )));
                self.set_loading(false);
                return;
            }
        };

        // 防御性重置：上次 agent 任务若 SubAgentEnd 因通道溢出被丢弃，
        // subagent_depth 会永久 > 0，导致所有后续 TokenUsageUpdate 被过滤（ctx 显示为 0）
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .subagent_depth = 0;
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_replied = false;
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .reconcile_already_done = false;
        // 清理后台任务 continuation 状态（用户主动发消息时覆盖自动 continuation）
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_done_pending_bg = false;
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pending_bg_continuation = None;
        // 保存原始用户输入（compact 后自动 re-submit 用）并重置 re-submit 计数器
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .last_user_input = Some(input.clone());
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .auto_compact_resubmit_count = 0;
        // 重置 LSP 诊断计数
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .lsp_errors = 0;
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .lsp_warnings = 0;
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .lsp_files_with_errors = 0;

        let (tx, rx) = mpsc::channel(256);
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_rx = Some(rx);

        // 创建取消令牌（Ctrl+C 触发中断）
        let cancel = AgentCancellationToken::new();
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .cancel_token = Some(cancel.clone());

        // 注意：HITL 审批和 AskUser 问答现在统一通过 TuiInteractionBroker 路由到 tx channel，
        // YOLO 模式由 HumanInTheLoopMiddleware::from_env() 内部处理（自动放行）。

        let cwd = self.services.cwd.clone();

        // 构建多模态 AgentInput（有附件时包含图片 blocks）
        let agent_input = if attachments.is_empty() {
            AgentInput::text(input.clone())
        } else {
            let mut blocks = vec![ContentBlock::text(input.clone())];
            for att in &attachments {
                blocks.push(ContentBlock::image_base64(
                    &att.media_type,
                    &att.base64_data,
                ));
            }
            AgentInput::blocks(MessageContent::blocks(blocks))
        };

        // 解析消息中的 /skill-name（字母、数字、连字符、下划线）
        let preload_skills = extract_skill_tokens(&input);

        // 确保当前 thread 存在
        let thread_id = self.ensure_thread_id();

        // 懒加载 Thread 级 LangfuseSession（首轮创建，后续复用；未配置环境变量时静默跳过）
        if self.session_mgr.sessions[self.session_mgr.active]
            .langfuse
            .langfuse_session
            .is_none()
        {
            tracing::debug!(thread_id = %thread_id, "langfuse: session is None, attempting to create");
            if let Some(cfg) = crate::langfuse::LangfuseConfig::from_env() {
                tracing::debug!(host = %cfg.host, "langfuse: config found, creating session");
                let session_id = thread_id.clone();
                let session = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(crate::langfuse::LangfuseSession::new(cfg, session_id))
                });
                if session.is_some() {
                    tracing::info!(thread_id = %thread_id, "langfuse: session created successfully");
                } else {
                    tracing::warn!(thread_id = %thread_id, "langfuse: session creation failed (None)");
                }
                self.session_mgr.sessions[self.session_mgr.active]
                    .langfuse
                    .langfuse_session = session.map(Arc::new);
            } else {
                tracing::debug!("langfuse: no config found in env, skipping session creation");
            }
        } else {
            tracing::debug!(thread_id = %thread_id, "langfuse: reusing existing session");
        }

        // 构造当前轮次的 Langfuse Tracer（同步，复用共享 Session）
        let langfuse_tracer = self.session_mgr.sessions[self.session_mgr.active]
            .langfuse
            .langfuse_session
            .clone()
            .map(|session| {
                let mut t = crate::langfuse::LangfuseTracer::new(session);
                t.on_trace_start(input.trim());
                Arc::new(parking_lot::Mutex::new(t))
            });
        self.session_mgr.sessions[self.session_mgr.active]
            .langfuse
            .langfuse_tracer = langfuse_tracer.clone();

        let span = tracing::info_span!(
            "thread.run",
            thread.id = %thread_id,
            thread.cwd = %cwd,
        );
        let history = self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_state_messages
            .clone();
        let agent_id = self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_id
            .clone();
        let thread_store = self.services.thread_store.clone();
        let thread_id_for_agent = thread_id.clone();
        let peri_config_for_agent = Arc::new(self.services.peri_config.clone().unwrap_or_default());
        let cron_scheduler = Some(self.services.cron.scheduler.clone());
        let permission_mode = self.services.permission_mode.clone();

        let mcp_pool = self.services.mcp_pool.clone();
        let plugin_skill_dirs = self
            .services
            .plugin_data
            .as_ref()
            .map(|pd| pd.all_skill_dirs.clone())
            .unwrap_or_default();
        let plugin_agent_dirs = self
            .services
            .plugin_data
            .as_ref()
            .map(|pd| pd.all_agent_dirs.clone())
            .unwrap_or_default();
        let plugin_lsp_servers = self
            .services
            .plugin_data
            .as_ref()
            .map(|pd| pd.all_lsp_servers.clone())
            .unwrap_or_default();
        let mut plugin_hooks = self
            .services
            .plugin_data
            .as_ref()
            .map(|pd| pd.all_hooks.clone())
            .unwrap_or_default();
        let local_hooks =
            rust_agent_middlewares::hooks::loader::load_settings_local_hooks(&self.services.cwd);

        // hook_groups：每组对应一个独立的 HookMiddleware 实例
        // plugin hooks 和 settings.local hooks 分组，便于独立控制
        let mut hook_groups: Vec<Vec<rust_agent_middlewares::hooks::RegisteredHook>> = Vec::new();
        if !plugin_hooks.is_empty() {
            tracing::info!(count = plugin_hooks.len(), "Registering plugin hooks");
            hook_groups.push(std::mem::take(&mut plugin_hooks));
        }
        if !local_hooks.is_empty() {
            tracing::info!(
                count = local_hooks.len(),
                "Registering settings.local hooks"
            );
            hook_groups.push(local_hooks);
        }

        // 扁平化所有 hooks 供 SubAgentTool 和 SessionEnd 使用
        let all_hooks: Vec<rust_agent_middlewares::hooks::RegisteredHook> =
            hook_groups.iter().flatten().cloned().collect();

        tracing::info!(
            groups = hook_groups.len(),
            total = all_hooks.len(),
            "Hook groups assembled for agent"
        );

        let hook_session_start = history.is_empty();

        tokio::spawn(
            async move {
                agent::run_universal_agent(agent::AgentRunConfig {
                    provider,
                    input: agent_input,
                    cwd,
                    history,
                    tx,
                    cancel,
                    agent_id,
                    langfuse_tracer,
                    thread_store,
                    thread_id: thread_id_for_agent,
                    preload_skills,
                    config: peri_config_for_agent,
                    cron_scheduler,
                    permission_mode,
                    mcp_pool,
                    plugin_skill_dirs,
                    plugin_agent_dirs,
                    plugin_hooks: all_hooks,
                    plugin_lsp_servers,
                    hook_groups,
                    hook_session_start,
                })
                .await;
            }
            .instrument(span),
        );
    }

    /// 发送缓冲的 cron 消息（每次只发一条，其余留待后续 Done 周期发送）
    /// 多条独立 cron 任务不应合并为一个 LLM 消息，避免语义混淆
    pub(crate) fn flush_pending_messages(&mut self) {
        if let Some(msg) = self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .pending_messages
            .first()
            .cloned()
        {
            self.session_mgr.sessions[self.session_mgr.active]
                .messages
                .pending_messages
                .remove(0);
            self.submit_message(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preload_skills_extracts_slash_prefix() {
        let result = extract_skill_tokens("请使用 /commit 提交");
        assert_eq!(result, vec!["commit"]);
    }

    #[test]
    fn test_preload_skills_extracts_multiple_skills() {
        let result = extract_skill_tokens("/review /refactor");
        assert_eq!(result, vec!["review", "refactor"]);
    }

    #[test]
    fn test_preload_skills_ignores_hash_prefix() {
        let result = extract_skill_tokens("#old-skill /new-skill");
        assert_eq!(result, vec!["new-skill"], "# 前缀不再匹配");
    }

    #[test]
    fn test_preload_skills_empty_for_no_skills() {
        let result = extract_skill_tokens("普通消息没有 skill 引用");
        assert!(result.is_empty());
    }

    #[test]
    fn test_preload_skills_truncates_on_invalid_char() {
        let result = extract_skill_tokens("/skill-name!suffix");
        assert_eq!(result, vec!["skill-name"], "遇到 ! 截断");
    }
}
