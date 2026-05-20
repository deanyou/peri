# /compact 命令作为普通文本发给 LLM，未触发压缩

**状态**：Open  
**优先级**：中  
**创建日期**：2026-05-20  

## 问题描述

在 TUI 中输入 `/compact` 命令后，消息被当作普通用户文本发送给 LLM。LLM 收到 `/compact` 字符串并尝试像普通对话一样回复，但没有触发任何上下文压缩操作。用户期望 `/compact` 能像 Claude Code 一样触发一轮 full compact。

## 症状详情

| 操作 | 预期 | 实际 |
|------|------|------|
| 输入 `/compact` 回车 | 触发 full compact（LLM 生成摘要 + 替换历史消息） | LLM 收到 `/compact` 文本，当作对话回复 |
| auto-compact（token 超过 0.85 阈值） | 正常触发 compact | 正常 |

**复现条件**：

- **复现频率**：必现
- **触发步骤**：
  1. 在 TUI 输入框中输入 `/compact`
  2. 按 Enter 发送
  3. Agent 启动，LLM 将 `/compact` 作为用户消息处理
  4. 没有任何 compact 行为发生
- **环境**：任意模型、任意上下文大小

## 涉及文件

- `peri-tui/src/command/session/compact.rs` — `/compact` 命令处理器，调用 `app.submit_message("/compact")` 将命令作为普通消息发送
- `peri-tui/src/app/agent_submit.rs` — `submit_message()` 通过 ACP `session/prompt` 发送用户消息
- `peri-middlewares/src/compact_middleware.rs` — `CompactMiddleware`，仅在 `before_model()` 中基于 token 阈值触发 auto-compact，不处理 `/compact` 消息
- `peri-acp/src/session/executor.rs` — `execute_prompt()` 中 auto-compact 循环，仅在 executor 内部按阈值触发
