# ACP 事件对齐存在多处缺口

**状态**：Fixed
**优先级**：中
**创建日期**：2026-05-16
**修复日期**：2026-05-16

## 问题描述

perihelion 的 ACP Agent 实现（`peri-tui/src/acp/`）在运行时事件映射上存在多处缺口。核心流程（TextChunk→AgentMessageChunk、ToolStart→ToolCall、ToolEnd→ToolCallUpdate）已正确实现，但辅助通知、字段级细节和 StopReason 映射不完整。IDE 客户端（如 Cursor）无法感知 token 消耗、模式变更、配置更新、命令列表等运行时状态。

## 症状详情

### 1. SessionUpdate 通知缺失（6/11 未实现）

以下 ACP `SessionUpdate` 变体从未从 Agent 推送给 Client：

| 缺失变体 | 应触发时机 | 影响 |
|----------|-----------|------|
| `UsageUpdate` | 每次 LLM 调用结束后（`LlmCallEnd`） | IDE 无法展示 token 消耗 |
| `CurrentModeUpdate` | `set_mode` / `set_config_option(mode)` 变更后 | 多客户端模式不同步 |
| `ConfigOptionUpdate` | `set_config_option` 变更后 | 配置变更后客户端不感知 |
| `SessionInfoUpdate` | prompt 完成后（标题/状态更新） | IDE 无法更新会话标题 |
| `AvailableCommandsUpdate` | session/new 后或命令列表变更时 | IDE 端的命令补全不可用 |

`UserMessageChunk` 是上行消息（Client→Agent），Agent 不需要发送，合理缺失。

### 2. ToolCall / ToolCallUpdate 字段缺失

| 缺失字段 | 应出现在 | 数据来源 |
|----------|---------|---------|
| `raw_input` | `ToolCall`（ToolStart） | `ExecutorEvent::ToolStart.input` |
| `raw_output` | `ToolCallUpdate`（ToolEnd） | `ExecutorEvent::ToolEnd.output`（尝试解析为 JSON） |
| `locations` | 两者 | 从工具参数/输出中提取文件路径 |

### 3. ExecutorEvent 映射缺失

| 未映射的 ExecutorEvent | 应映射为 | 原因 |
|------------------------|---------|------|
| `LlmCallEnd` | `UsageUpdate` | 包含 token 用量 |
| `ContextWarning` | `UsageUpdate` | 上下文窗口告警 |
| `LlmRetrying` | `SessionInfoUpdate` | 重试状态应告知用户 |

`StepDone`、`StateSnapshot`、`MessageAdded`、`LlmCallStart`、`SubagentStarted/Stopped`、`CompactStarted/Completed` 等无需映射。

### 4. ToolKind 推断不完整

`event_mapper.rs:80-88` 的 `infer_tool_kind()` 函数：

- `WebFetch` / `WebSearch` → 应映射为 `Fetch`，当前为 `Other`
- `Agent` → 可考虑 `Think`
- `AskUserQuestion` → 可考虑 `Think`
- `TodoWrite` → 可考虑 `Think`

### 5. StopReason 映射不完整

`dispatch.rs:441-448` 的 stop_reason 映射：

| AgentError | 当前映射 | 正确映射 |
|---|---|---|
| `Ok(())` | `EndTurn` ✅ | |
| `Err(Interrupted)` | `Cancelled` ✅ | |
| `Err(Refusal)` | `EndTurn` ❌ | `Refusal` |
| `Err(MaxTokens)` | `EndTurn` ❌ | `MaxTokens` |
| `Err(MaxIterations)` | `EndTurn` ❌ | `MaxTurnRequests` |

## 涉及文件

- `peri-tui/src/acp/event_mapper.rs` —— 事件→SessionUpdate 映射函数（缺口 1、2、3、4）
- `peri-tui/src/acp/dispatch.rs` —— prompt handler + mode/config handler（缺口 1、5）
- `peri-tui/src/acp/broker.rs` —— 权限桥接（HITL→RequestPermission，已正确实现）
- `peri-tui/src/acp/agent_assembler.rs` —— Agent 组装（中间件链构建，无缺口）
