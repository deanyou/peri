# 流式渲染视觉问题：多轮文本合并 + 只读工具过度折叠

**状态**：Open
**优先级**：中
**创建日期**：2026-05-13

## 问题描述

流式渲染过程中存在两个视觉问题：(1) 多轮 AI 回复文本被合并在同一个 AssistantBubble 中，缺乏轮次分隔；(2) `aggregate_tool_groups` 将相邻只读工具（Read/Grep/Glob/AskUserQuestion）折叠为 ToolCallGroup，用户期望每个工具调用独立显示。

## 症状详情

### 问题 1：多轮 AI 文本合并在一个气泡

| 维度 | 描述 |
|------|------|
| 观察时机 | 流式过程中即可观察到（非 Done 后回看） |
| 复现条件 | 正常对话即可复现，无需特殊场景 |
| 期望行为 | 每个 ReAct 轮次的 AI 文本应在独立的 AssistantBubble 中显示 |
| 实际行为 | 多轮文本堆在一个气泡里，工具调用穿插其中 |

### 问题 2：只读工具过度折叠

| 维度 | 描述 |
|------|------|
| 观察时机 | 任何包含多个只读工具调用的对话 |
| 期望行为 | 每个 Read/Grep/Glob 调用独立显示为 ToolBlock |
| 实际行为 | 相邻只读工具被 `aggregate_tool_groups` 合并为 ToolCallGroup（如 "Read 3 files, Searched for 2 patterns"） |

## 相关代码

### 问题 1 相关

- `rust-agent-tui/src/app/message_pipeline.rs:561-592` — `build_streaming_bubble()` 将所有 `current_ai_text` 放入一个 AssistantBubble，不按工具调用边界分割文本
- `rust-agent-tui/src/app/message_pipeline.rs:641-736` — `build_tail_vms()` 构建尾部 VMs，streaming bubble 作为单一 VM 追加
- `rust-agent-tui/src/app/message_pipeline.rs:744-753` — `set_completed()` 在 StateSnapshot 到达时清空 `current_ai_text`
- `rust-create-agent/src/agent/executor/final_answer.rs:38-53` — `emit_snapshot_and_drain_notifications()` 每次工具调用后发射 StateSnapshot

### 问题 2 相关

- `rust-agent-tui/src/ui/message_view.rs:138-208` — `aggregate_tool_groups()` / `aggregate_tail_tool_groups()` 合并相邻只读 ToolBlock 为 ToolCallGroup
- `rust-agent-tui/src/ui/message_view.rs:22-28` — `ToolCategory` 定义了 4 种只读工具分类（Read/Search/Glob/AskUser）
- `rust-agent-tui/src/app/message_pipeline.rs:733` — `build_tail_vms()` 末尾调用 `aggregate_tool_groups()`
- `rust-agent-tui/src/app/message_pipeline.rs:803` — `messages_to_view_models()` 末尾也调用 `aggregate_tool_groups()`

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 发起一个需要多轮工具调用的请求（如 "帮我分析这个项目的结构"）
  2. 观察 AI 文本和工具调用的显示方式
  3. 注意多轮 AI 文本是否在同一个气泡中
  4. 注意只读工具是否被折叠为 ToolCallGroup
