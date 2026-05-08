use ratatui::layout::Rect;

use super::ChatSession;

/// 会话管理器：管理多个 ChatSession 实例和当前激活索引。
pub struct SessionManager {
    pub sessions: Vec<ChatSession>,
    pub active: usize,
    pub session_areas: Vec<Rect>,
}

impl SessionManager {
    pub fn new(initial_session: ChatSession) -> Self {
        Self {
            sessions: vec![initial_session],
            active: 0,
            session_areas: Vec::new(),
        }
    }

    /// 获取当前激活 session 的不可变引用
    pub fn current(&self) -> &ChatSession {
        &self.sessions[self.active]
    }

    /// 获取当前激活 session 的可变引用
    pub fn current_mut(&mut self) -> &mut ChatSession {
        &mut self.sessions[self.active]
    }

    pub fn session_at(&self, idx: usize) -> Option<&ChatSession> {
        self.sessions.get(idx)
    }

    pub fn session_at_mut(&mut self, idx: usize) -> Option<&mut ChatSession> {
        self.sessions.get_mut(idx)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}
