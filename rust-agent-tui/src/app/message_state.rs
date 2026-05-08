use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::{mpsc, Notify};

use crate::ui::message_view::MessageViewModel;
use crate::ui::render_thread::{RenderCache, RenderEvent};

use super::message_pipeline::MessagePipeline;

/// 消息状态：会话级的消息管线、渲染通道、消息列表。
pub struct MessageState {
    pub view_messages: Vec<MessageViewModel>,
    pub round_start_vm_idx: usize,
    pub pipeline: MessagePipeline,
    pub render_tx: mpsc::UnboundedSender<RenderEvent>,
    pub render_cache: Arc<RwLock<RenderCache>>,
    pub render_notify: Arc<Notify>,
    pub last_render_version: u64,
    pub pending_messages: Vec<String>,
    /// 最近一次提交的用户文本（用于 Ctrl+C 中断时恢复到输入框）
    pub last_submitted_text: Option<String>,
}

impl MessageState {
    pub fn new(
        cwd: String,
        render_tx: mpsc::UnboundedSender<RenderEvent>,
        render_cache: Arc<RwLock<RenderCache>>,
        render_notify: Arc<Notify>,
    ) -> Self {
        Self {
            view_messages: Vec::new(),
            round_start_vm_idx: 0,
            pipeline: MessagePipeline::new(cwd),
            render_tx,
            render_cache,
            render_notify,
            last_render_version: 0,
            pending_messages: Vec::new(),
            last_submitted_text: None,
        }
    }
}
