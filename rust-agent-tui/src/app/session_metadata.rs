use super::hitl_prompt::PendingAttachment;

/// 会话元数据：低频访问的会话状态。
pub struct SessionMetadata {
    pub pending_attachments: Vec<PendingAttachment>,
    pub last_human_message: Option<String>,
    pub pre_submit_state_len: usize,
}

impl SessionMetadata {
    pub fn new() -> Self {
        Self {
            pending_attachments: Vec::new(),
            last_human_message: None,
            pre_submit_state_len: 0,
        }
    }
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self::new()
    }
}
