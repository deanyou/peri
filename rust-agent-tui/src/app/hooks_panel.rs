use rust_agent_middlewares::hooks::types::{HookEvent, HookType, RegisteredHook};

/// Hook 事件描述信息
struct HookEventInfo {
    event: HookEvent,
    display_name: String,
    description: &'static str,
}

fn build_event_table() -> Vec<HookEventInfo> {
    let known = &[
        (HookEvent::PreToolUse, "PreToolUse", "Before tool execution"),
        (
            HookEvent::PostToolUse,
            "PostToolUse",
            "After tool execution",
        ),
        (
            HookEvent::PostToolUseFailure,
            "PostToolUseFailure",
            "After tool execution fails",
        ),
        (
            HookEvent::PermissionRequest,
            "PermissionRequest",
            "Before auto mode classifier decides",
        ),
        (
            HookEvent::UserPromptSubmit,
            "UserPromptSubmit",
            "When user submits a prompt",
        ),
        (
            HookEvent::SessionStart,
            "SessionStart",
            "When a new session starts",
        ),
        (HookEvent::SessionEnd, "SessionEnd", "When a session ends"),
        (HookEvent::Stop, "Stop", "When agent stops"),
        (
            HookEvent::StopFailure,
            "StopFailure",
            "When agent stops with failure",
        ),
        (
            HookEvent::SubagentStart,
            "SubagentStart",
            "When a subagent starts",
        ),
        (
            HookEvent::SubagentStop,
            "SubagentStop",
            "When a subagent stops",
        ),
        (
            HookEvent::PreCompact,
            "PreCompact",
            "Before context compaction",
        ),
        (
            HookEvent::PostCompact,
            "PostCompact",
            "After context compaction",
        ),
        (
            HookEvent::Notification,
            "Notification",
            "When agent needs user input",
        ),
    ];
    known
        .iter()
        .map(|(e, n, d)| HookEventInfo {
            event: e.clone(),
            display_name: n.to_string(),
            description: d,
        })
        .collect()
}

/// 面板列表中的一行（事件 + 该事件下注册的 hooks 汇总）
#[derive(Debug, Clone)]
pub struct HookEventEntry {
    pub event: HookEvent,
    pub display_name: String,
    pub description: String,
    pub hook_count: usize,
    /// 该事件下注册的所有 hook 详情
    pub hooks: Vec<HookDetail>,
}

#[derive(Debug, Clone)]
pub struct HookDetail {
    pub hook_type: HookType,
    pub matcher: Option<String>,
    pub plugin_name: String,
}

#[derive(Clone)]
pub struct HooksPanel {
    /// 所有事件条目（仅包含有 hooks 的事件）
    pub entries: Vec<HookEventEntry>,
    /// 光标位置（entries 索引）
    pub cursor: usize,
    /// 内容滚动偏移（以渲染行为单位）
    pub scroll_offset: u16,
}

/// 计算单个 hook 详情占几行（type+summary 行 + 可选 matcher 行 + plugin 行）
fn detail_lines(detail: &HookDetail) -> u16 {
    1 + if detail.matcher.is_some() { 1 } else { 0 } + 1
}

impl HooksPanel {
    pub fn new(registered_hooks: Vec<RegisteredHook>) -> Self {
        let event_table = build_event_table();

        // 收集所有出现的事件（去重），保持首次出现顺序
        let mut event_order: Vec<HookEvent> = Vec::new();
        for rh in &registered_hooks {
            if !event_order.contains(&rh.event) {
                event_order.push(rh.event.clone());
            }
        }

        let mut entries = Vec::new();

        for event in &event_order {
            let event_hooks: Vec<HookDetail> = registered_hooks
                .iter()
                .filter(|rh| &rh.event == event)
                .map(|rh| HookDetail {
                    hook_type: rh.hook.clone(),
                    matcher: rh.matcher.clone(),
                    plugin_name: rh.plugin_name.clone(),
                })
                .collect();

            if event_hooks.is_empty() {
                continue;
            }

            // 查找已知事件的描述
            let (display_name, description) = event_table
                .iter()
                .find(|info| &info.event == event)
                .map(|info| (info.display_name.clone(), info.description.to_string()))
                .unwrap_or_else(|| {
                    let name = match event {
                        HookEvent::Unknown(s) => s.clone(),
                        other => format!("{:?}", other),
                    };
                    (name, String::new())
                });

            entries.push(HookEventEntry {
                event: event.clone(),
                display_name,
                description,
                hook_count: event_hooks.len(),
                hooks: event_hooks,
            });
        }

        Self {
            entries,
            cursor: 0,
            scroll_offset: 0,
        }
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn total_hooks(&self) -> usize {
        self.entries.iter().map(|e| e.hook_count).sum()
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let total = self.total();
        if total == 0 {
            return;
        }
        self.cursor = ((self.cursor as isize + delta).rem_euclid(total as isize)) as usize;
    }

    /// 当前选中的事件条目
    pub fn current_entry(&self) -> Option<&HookEventEntry> {
        self.entries.get(self.cursor)
    }

    /// 固定头部行数（统计行 + 提示行 + 空行）
    pub fn header_lines(&self) -> u16 {
        if self.entries.is_empty() {
            2 // "none configured" + 空行
        } else {
            3 // 统计行 + 提示行 + 空行
        }
    }

    /// 当前光标 entry 对应的实际渲染行号（0-based）
    pub fn cursor_line(&self) -> u16 {
        let mut line = self.header_lines();
        // 每个 entry 前面有一个事件头行
        // 光标 entry 之前的所有 entry 各占 1 行（不展开）
        line += self.cursor as u16;
        // 如果光标在前面，前面没有展开详情，但光标 entry 本身展开
        // 展开详情从光标行之后开始，光标行就是事件头行
        line
    }

    /// 当前光标 entry 展开后的总行数（事件头 + 详情 + 空行）
    pub fn expanded_lines(&self) -> u16 {
        let entry = match self.entries.get(self.cursor) {
            Some(e) => e,
            None => return 0,
        };
        let detail: u16 = entry.hooks.iter().map(detail_lines).sum();
        1 + detail + 1 // 事件头 + 详情行 + 空行
    }

    /// 整个面板的内容总行数
    pub fn total_content_lines(&self) -> u16 {
        let mut h = self.header_lines();
        for _entry in &self.entries {
            h += 1; // 事件头行
        }
        // 加上当前展开的详情行
        if let Some(entry) = self.entries.get(self.cursor) {
            let detail: u16 = entry.hooks.iter().map(detail_lines).sum();
            h += detail + 1; // 详情行 + 空行
        }
        h
    }
}

/// 从 HookType 提取简短的类型标签（用于渲染）
pub fn hook_type_label(hook: &HookType) -> &'static str {
    match hook {
        HookType::Command { .. } => "command",
        HookType::Prompt { .. } => "prompt",
        HookType::Http { .. } => "http",
        HookType::Agent { .. } => "agent",
    }
}

/// 从 HookType 提取简短的执行内容摘要
pub fn hook_type_summary(hook: &HookType) -> String {
    match hook {
        HookType::Command { command, .. } => {
            let cmd: String = command.chars().take(40).collect();
            if command.chars().count() > 40 {
                format!("{}...", cmd)
            } else {
                cmd
            }
        }
        HookType::Prompt { prompt, .. } => {
            let p: String = prompt.chars().take(40).collect();
            if prompt.chars().count() > 40 {
                format!("{}...", p)
            } else {
                p
            }
        }
        HookType::Http { url, .. } => {
            let u: String = url.chars().take(40).collect();
            if url.chars().count() > 40 {
                format!("{}...", u)
            } else {
                u
            }
        }
        HookType::Agent { prompt, .. } => {
            let p: String = prompt.chars().take(40).collect();
            if prompt.chars().count() > 40 {
                format!("{}...", p)
            } else {
                p
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_hook(event: HookEvent, matcher: Option<&str>) -> RegisteredHook {
        let hook_type: HookType = serde_json::from_value(serde_json::json!({
            "type": "command",
            "command": "echo test"
        }))
        .unwrap();
        RegisteredHook {
            hook: hook_type,
            event,
            matcher: matcher.map(String::from),
            plugin_name: "test".to_string(),
            plugin_id: "test".to_string(),
            plugin_root: PathBuf::from("/tmp"),
            plugin_data_dir: PathBuf::from("/tmp"),
            plugin_options: HashMap::new(),
        }
    }

    #[test]
    fn test_cursor_line_basic() {
        // 3 entries, cursor on 0 → header(3) + 0 = 3
        let hooks = vec![
            make_hook(HookEvent::PreToolUse, None),
            make_hook(HookEvent::PostToolUse, None),
            make_hook(HookEvent::Stop, None),
        ];
        let panel = HooksPanel::new(hooks);
        assert_eq!(panel.cursor_line(), 3); // header=3, cursor=0 → line 3
    }

    #[test]
    fn test_cursor_line_middle() {
        // 3 entries, cursor on 1 → header(3) + 1 = 4
        let hooks = vec![
            make_hook(HookEvent::PreToolUse, None),
            make_hook(HookEvent::PostToolUse, None),
            make_hook(HookEvent::Stop, None),
        ];
        let mut panel = HooksPanel::new(hooks);
        panel.cursor = 1;
        assert_eq!(panel.cursor_line(), 4); // header=3, cursor=1 → line 4
    }

    #[test]
    fn test_expanded_lines_with_matcher() {
        let hook_type: HookType = serde_json::from_value(serde_json::json!({
            "type": "command",
            "command": "echo test"
        }))
        .unwrap();
        let hook = RegisteredHook {
            hook: hook_type,
            event: HookEvent::PreToolUse,
            matcher: Some("Bash".to_string()),
            plugin_name: "test".to_string(),
            plugin_id: "test".to_string(),
            plugin_root: PathBuf::from("/tmp"),
            plugin_data_dir: PathBuf::from("/tmp"),
            plugin_options: HashMap::new(),
        };
        let panel = HooksPanel::new(vec![hook]);
        // 1 event header + 1 detail(type+summary=1, matcher=1, plugin=1 → 3) + 1 empty = 5
        assert_eq!(panel.expanded_lines(), 5);
    }

    #[test]
    fn test_expanded_lines_without_matcher() {
        let hooks = vec![make_hook(HookEvent::PreToolUse, None)];
        let panel = HooksPanel::new(hooks);
        // 1 event header + 1 detail(type+summary=1, no matcher, plugin=1 → 2) + 1 empty = 4
        assert_eq!(panel.expanded_lines(), 4);
    }
}
