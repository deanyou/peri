/// 后台任务完成通知（注入到主 agent 消息流中）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackgroundTaskResult {
    pub task_id: String,
    pub agent_name: String,
    pub prompt_summary: String,
    pub success: bool,
    pub output: String,
    pub tool_calls_count: usize,
    pub duration_ms: u64,
}

/// Agent 执行过程中的增量事件
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// AI 推理内容（reasoning/思考过程）
    AiReasoning(String),
    /// LLM 输出最终文字（非流式，整段答案），携带所属 AI 消息的 message_id
    TextChunk {
        message_id: crate::messages::MessageId,
        chunk: String,
    },
    /// 工具调用开始（工具名 + 参数），携带所属 AI 消息的 message_id
    ToolStart {
        message_id: crate::messages::MessageId,
        tool_call_id: String,
        name: String,
        input: serde_json::Value,
    },
    /// 工具调用结束（结果或错误），携带所属 AI 消息的 message_id
    ToolEnd {
        message_id: crate::messages::MessageId,
        tool_call_id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    /// 一轮 ReAct 步骤完成
    StepDone { step: usize },
    /// 状态快照（含完整的消息历史），用于持久化和断点续跑
    StateSnapshot(Vec<crate::messages::BaseMessage>),
    /// 增量消息（BaseMessage），持久化和遥测的最小数据单元
    MessageAdded(crate::messages::BaseMessage),
    /// LLM 调用开始（携带完整 input messages 快照 + 工具定义，用于 Langfuse Generation）
    LlmCallStart {
        step: usize,
        messages: Vec<crate::messages::BaseMessage>,
        tools: Vec<crate::tools::ToolDefinition>,
    },
    /// LLM 调用结束（携带模型名、输出文本、token 使用量）
    LlmCallEnd {
        step: usize,
        model: String,
        output: String,
        usage: Option<crate::llm::types::TokenUsage>,
    },
    /// 上下文窗口使用警告（阈值触发时发出）
    ContextWarning {
        used_tokens: u64,
        total_tokens: u64,
        percentage: f64,
    },
    /// LLM 调用重试中
    LlmRetrying {
        attempt: usize,
        max_attempts: usize,
        delay_ms: u64,
        error: String,
    },
    /// 后台 agent 任务完成（TUI 使用，用于空闲时通知）
    BackgroundTaskCompleted(BackgroundTaskResult),
    /// 子 agent 开始执行
    SubagentStarted { agent_name: String },
    /// 子 agent 执行完成
    SubagentStopped { agent_name: String, result: String },
    /// Session 结束
    SessionEnded,
    /// 上下文压缩开始
    CompactStarted,
    /// 上下文压缩完成
    CompactCompleted,
}

/// 事件回调 trait（应用层实现）
///
/// 在 `ReActAgent` 执行过程中，关键节点会调用 `on_event`。
/// 实现者通过 `mpsc::Sender` 等机制将事件转发给 UI 层。
pub trait AgentEventHandler: Send + Sync {
    fn on_event(&self, event: AgentEvent);
}

/// 函数闭包适配器 —— 方便快速实现 `AgentEventHandler`
///
/// # 示例
/// ```rust,ignore
/// let tx = tx.clone();
/// let handler = FnEventHandler(move |event| {
///     let _ = tx.try_send(event);
/// });
/// executor.with_event_handler(Arc::new(handler))
/// ```
pub struct FnEventHandler<F>(pub F)
where
    F: Fn(AgentEvent) + Send + Sync;

impl<F> AgentEventHandler for FnEventHandler<F>
where
    F: Fn(AgentEvent) + Send + Sync,
{
    fn on_event(&self, event: AgentEvent) {
        (self.0)(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_warning_serde_roundtrip() {
        let ev = AgentEvent::ContextWarning {
            used_tokens: 150000,
            total_tokens: 200000,
            percentage: 75.0,
        };
        let json = serde_json::to_string(&ev).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEvent::ContextWarning {
            used_tokens,
            total_tokens,
            percentage,
        } = deserialized
        {
            assert_eq!(used_tokens, 150000);
            assert_eq!(total_tokens, 200000);
            assert!((percentage - 75.0).abs() < 0.01);
        } else {
            panic!("Deserialized to wrong variant");
        }
    }

    #[test]
    fn test_llm_retrying_serde_roundtrip() {
        let ev = AgentEvent::LlmRetrying {
            attempt: 2,
            max_attempts: 5,
            delay_ms: 2000,
            error: "API 错误 503: Service Unavailable".to_string(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEvent::LlmRetrying {
            attempt,
            max_attempts,
            delay_ms,
            error,
        } = deserialized
        {
            assert_eq!(attempt, 2);
            assert_eq!(max_attempts, 5);
            assert_eq!(delay_ms, 2000);
            assert_eq!(error, "API 错误 503: Service Unavailable");
        } else {
            panic!("Deserialized to wrong variant");
        }
    }

    #[test]
    fn test_subagent_started_serde_roundtrip() {
        let ev = AgentEvent::SubagentStarted {
            agent_name: "test-agent".to_string(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""type":"subagent_started""#));
        assert!(json.contains(r#""agent_name":"test-agent""#));
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEvent::SubagentStarted { agent_name } = deserialized {
            assert_eq!(agent_name, "test-agent");
        } else {
            panic!("Deserialized to wrong variant");
        }
    }

    #[test]
    fn test_subagent_stopped_serde_roundtrip() {
        let ev = AgentEvent::SubagentStopped {
            agent_name: "test-agent".to_string(),
            result: "done".to_string(),
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""type":"subagent_stopped""#));
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        if let AgentEvent::SubagentStopped { agent_name, result } = deserialized {
            assert_eq!(agent_name, "test-agent");
            assert_eq!(result, "done");
        } else {
            panic!("Deserialized to wrong variant");
        }
    }

    #[test]
    fn test_session_ended_serde() {
        let ev = AgentEvent::SessionEnded;
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains(r#""type":"session_ended""#));
        let deserialized: AgentEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, AgentEvent::SessionEnded));
    }

    #[test]
    fn test_compact_events_serde() {
        let ev1 = AgentEvent::CompactStarted;
        let json1 = serde_json::to_string(&ev1).unwrap();
        assert!(json1.contains(r#""type":"compact_started""#));

        let ev2 = AgentEvent::CompactCompleted;
        let json2 = serde_json::to_string(&ev2).unwrap();
        assert!(json2.contains(r#""type":"compact_completed""#));

        let d1: AgentEvent = serde_json::from_str(&json1).unwrap();
        assert!(matches!(d1, AgentEvent::CompactStarted));
        let d2: AgentEvent = serde_json::from_str(&json2).unwrap();
        assert!(matches!(d2, AgentEvent::CompactCompleted));
    }
}
