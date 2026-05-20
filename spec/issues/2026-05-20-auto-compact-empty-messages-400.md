# Auto compact 后 LLM 请求 messages 为空导致 400 错误

**状态**：Open
**优先级**：高
**创建日期**：2026-05-20

## 问题描述

上下文 token 超过阈值触发 auto compact 后，compact 完成紧接的 LLM 请求发送了空的 messages 数组，API 返回 400 错误 `messages: at least one message is required`。之后整个 session 卡死，无法继续对话。该问题在 DeepSeek 模型上必现。

## 症状详情

| 维度 | 表现 |
|------|------|
| 触发时机 | Auto compact（上下文 token 超过阈值自动触发） |
| Compact 表现 | TUI 显示「上下文已压缩」通知 |
| 错误信息 | `LLM HTTP 错误 (400): API 错误 400 Bad Request: messages: at least one message is required` |
| 错误后状态 | Session 完全卡死，无法继续输入或发送消息 |
| 复现频率 | 必现 |

### 错误时序

1. 用户正常对话，上下文逐渐增长
2. Token 超过 compact 阈值（默认 0.85），auto compact 触发
3. TUI 显示「✻ 上下文已压缩」
4. 紧接着显示「✗ Agent Error: LLM HTTP 错误 (400): API 错误 400 Bad Request: messages: at least one message is required」
5. Session 卡死

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 使用 DeepSeek 模型（通过 Anthropic 兼容端点）
  2. 持续对话直到上下文 token 超过 auto compact 阈值
  3. Auto compact 触发后立即出现 400 错误
- **环境**：DeepSeek 模型，Anthropic 兼容端点

## 相关 Issue

- `spec/issues/2026-05-20-llm-error-message-area-clear-flicker.md` — compact 后 LLM 400 错误导致 UI 清空（不同层面：UI 表现 vs API 请求层面 messages 为空）

## 涉及文件

- `peri-acp/src/session/executor.rs` — Agent 执行管线，compact 后 resubmit 逻辑所在
- `peri-acp/src/session/compact_runner.rs` — Full/micro compact 执行逻辑
