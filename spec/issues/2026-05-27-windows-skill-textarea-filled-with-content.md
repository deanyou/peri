# Windows 上输入 /skill-name 后 textarea 被 SKILL.md 原文填充

**状态**：Open
**优先级**：中
**创建日期**：2026-05-27

## 问题描述

在 Windows Terminal 上，用户在 textarea 中输入 `/skill-name` 并按 Enter 发送后，textarea 不像 macOS 上那样正常清空，而是被对应的 SKILL.md 文件的全部 markdown 原文内容填充。macOS 上行为正常（textarea 清空，skill 内容通过 SkillPreloadMiddleware 以 fake Read 工具调用注入到 agent 上下文）。

## 症状详情

| 平台 | 输入 | 期望行为 | 实际行为 |
|------|------|----------|----------|
| macOS | `/skill-name` + Enter | textarea 清空，skill 通过 SkillPreload 注入 | textarea 清空（正常） |
| Windows Terminal | `/skill-name` + Enter | textarea 清空，skill 通过 SkillPreload 注入 | textarea 被 SKILL.md 全文填充 |

- 所有 skill 均受影响（必现）
- textarea 中**只显示** SKILL.md 的原始 markdown 内容（可能几百行），原来的 `/skill-name` 已消失
- 纯键盘输入，无复制粘贴操作
- skill 本身是否被正确加载到 agent 上下文（通过 SkillPreloadMiddleware）未确认

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 在 Windows Terminal 中启动 peri-tui
  2. 在 textarea 中输入任意已知 skill 名称，如 `/code-review`
  3. 按 Enter 发送
- **环境**：Windows + Windows Terminal（macOS 不受影响）

## 代码分析

### 数据流追踪

代码路径（`normal_keys.rs:153-185`）：

```
1. text.starts_with('/') → true
2. textarea = build_textarea(false)  ← textarea 被清空
3. registry.dispatch(app, &text)     ← 尝试命令匹配
   ├─ known = true  → 命令已执行，结束
   └─ known = false → 继续 Skill 匹配
4. Skill 匹配成功 → return Action::Submit(text)
5. main.rs → submit_message(text)   ← 不写入 textarea
```

### 唯一可能的写入路径

全局搜索 `read_to_string.*textarea` 发现**只有一处**代码会读取文件内容并写入 textarea：

```rust
// plugin_command.rs:23-27
fn execute(&self, app: &mut App, _args: &str) {
    match &self.entry.source {
        CommandSource::Plugin { path } => {
            if let Ok(content) = std::fs::read_to_string(path) {
                app.active_mut().ui.textarea.insert_str(&content);
```

`PluginCommandAdapter::execute()` 读取**命令文件**并插入到 textarea。这是 Claude Code 的 slash command 设计——输入 `/command-name` 时把命令 markdown 文件内容填入 textarea 供用户编辑后发送。

### 假设：命令前缀匹配命中了 Skill 名称

`CommandRegistry::dispatch()`（`command/mod.rs:106-117`）有**前缀唯一匹配**逻辑：

```rust
// 3. 前缀唯一匹配（同时对 name 和 aliases）
let matches: Vec<_> = self.commands.iter()
    .filter(|c| c.name().starts_with(name) || ...)
    .collect();
if matches.len() == 1 {
    matches[0].execute(app, args);  // ← 执行 PluginCommandAdapter::execute
    return true;                     // ← known = true，不走到 Skill 匹配
}
```

如果某个 `PluginCommand` 的名称以 skill-name 开头，前缀匹配会命中它，导致 SKILL.md（如果被注册为命令文件）被读取并填入 textarea。

**但此假设有疑点**：插件命令名称格式为 `plugin_name:command_name`（如 `superpowers:test-driven-development`），而用户输入的是纯 skill 名（如 `/code-review`）。前缀匹配要求 `plugin_name:command_name` 以 `code-review` 开头，这在正常情况下不应成立。

### 待验证问题

1. **Windows 上是否有额外的插件命令被注册**，其名称恰好与 skill 名称前缀匹配？
2. **Windows 上的插件加载行为是否不同**，导致某些 .md 文件同时被注册为 command 和 skill？
3. **是否 Windows Terminal 的 `detect_simulated_paste`（`event/mod.rs:159`）误触发**，将 SKILL.md 内容通过剪贴板注入？（但用户确认未复制过，且纯键盘输入）

### 诊断建议

在 Windows 上添加以下临时日志以精确定位：

```rust
// normal_keys.rs:165（dispatch 前后）
tracing::info!(text = %text, "dispatching command");
// dispatch 后
tracing::info!(known = known, "dispatch result");

// plugin_command.rs:27（execute 中）
tracing::info!(path = %path.display(), content_len = content.len(), "PluginCommand inserting into textarea");

// event/mod.rs:196（detect_simulated_paste 中）
tracing::info!(text_len = text.len(), "detect_simulated_paste converted to Paste event");
```

## 涉及文件

- `peri-tui/src/event/keyboard/normal_keys.rs`（153-185 行）—— `/` 前缀输入的 Enter 处理：textarea 清空 + 命令匹配 + Skill 匹配
- `peri-tui/src/command/mod.rs`（87-120 行）—— `dispatch()` 命令匹配逻辑（含前缀唯一匹配）
- `peri-tui/src/command/session/plugin_command.rs`（23-27 行）—— **唯一读取文件并写入 textarea 的代码路径**
- `peri-tui/src/event/mod.rs`（159-197 行）—— `detect_simulated_paste` Windows 粘贴模拟检测
- `peri-middlewares/src/subagent/skill_preload.rs`（95-111 行）—— SkillPreloadMiddleware 注入
