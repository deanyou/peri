# OpenAI 兼容第三方 Provider 上下文用量计算不准确

**状态**：Fixed
**优先级**：高
**创建日期**：2026-05-11
**修复 commit**：`1497d5b` fix(context): sync context_window from model to TUI layer

## 问题描述

使用 OpenAI 兼容第三方 Provider 时，TUI 显示的上下文用量（203k/200K，101%）与 API 实际报告的 input token 数（~100k）存在约 2 倍差距。

## 症状详情

| 指标 | TUI 显示 | API 实际值 |
|------|---------|-----------|
| 上下文用量 | 203k（101%） | input ~100k |
| context_window | 200k（硬编码默认值） | 模型实际 256k |
| 缓存 token | 100755 | — |

- **环境**：OpenAI 兼容第三方 Provider，非推理模型
- **触发场景**：一次 prompt 的工具调用过程中，上下文持续增长
- **实际影响**：显示 101% 但 API 正常无报错

## 相关代码

- `rust-create-agent/src/agent/token.rs:38-44` —— `estimated_context_tokens()` 计算：`last_usage.input_tokens + last_usage.output_tokens`
- `rust-create-agent/src/agent/token.rs:21-36` —— `accumulate()` 方法：`last_usage` 更新逻辑
- `rust-create-agent/src/llm/openai.rs:557-576` —— OpenAI adapter 的 usage 解析：`input_tokens = prompt_tokens`，`cache_read = cached_tokens`
- `rust-create-agent/src/llm/openai.rs:90-105` —— `context_window_inner()` 硬编码模型名匹配，未知模型默认 200k
- `rust-create-agent/src/agent/executor/llm_step.rs:66-118` —— ContextWarning 发射逻辑，依赖 `estimated_context_tokens()` 和 `context_window`
- `rust-create-agent/src/agent/executor/mod.rs:256-272` —— ReAct 循环中的 micro-compact 检查
- `rust-agent-tui/src/ui/main_ui/panels/status.rs:130-138` —— `/context` 面板显示 `estimated_context_tokens()` 和百分比
- `rust-agent-tui/src/ui/main_ui/status_bar.rs:87-117` —— 状态栏 `ctx:` 显示
