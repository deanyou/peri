/// Random tips shown below the loading spinner, inspired by Claude Code.
const TIPS: &[&str] = &[
    "按 / 输入命令，Tab 补全",
    "Ctrl+C 中断 Agent，Shift+Tab 切换权限模式",
    "Alt+M 快速切换模型（opus / sonnet / haiku）",
    "Alt+Enter 在输入框中换行",
    "拖拽文件或图片到终端可自动附加到消息",
    "长按 Ctrl+V 粘贴剪贴板图片",
    "Ctrl+U/D 滚动消息历史，↑/↓ 浏览输入历史",
    "Ctrl+N/P 切换 Session，Ctrl+W 关闭",
    "Esc 关闭弹窗或面板，Enter 确认选择",
    "/compact 压缩上下文节省 token",
    "/clear 清空当前对话",
    "/model 切换 LLM 模型",
    "/history 浏览历史对话记录",
    "/loop 创建定时循环任务",
    "/plugin 管理 Claude Code 插件",
    "在 .claude/skills/ 中添加自定义 Skills",
    "在 .claude/agents/ 中定义 SubAgent",
    "对复杂任务让 Agent 先制定计划再执行",
];

/// Pick a tip based on a tick counter. Tip changes every ~180 ticks (roughly every 3 seconds at 60fps).
pub fn pick_tip(tick: u64) -> &'static str {
    TIPS[((tick / 180) as usize) % TIPS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("tips_test.rs");
}
