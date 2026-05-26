# 同步 SubAgent Ctrl+C 中断修复尝试汇总

**状态**：Open
**优先级**：中
**创建日期**：2026-05-26
**关联 Issue**：[#2026-05-25-ctrl-c-cannot-interrupt-sync-subagent](../spec/issues/2026-05-25-ctrl-c-cannot-interrupt-sync-subagent.md)

## 问题描述

针对「Ctrl+C 无法中断同步 SubAgent」问题进行了两次修复尝试，均未生效。用户反馈：`ctrl 应该没有传递到子 agent`——取消信号未传播到子 Agent 的取消令牌。

## 尝试记录

### 尝试 1：在 `SubAgentTool::invoke()` 同步路径添加 `tokio::select!`

**文件**：`peri-middlewares/src/subagent/tool/define.rs:1157`

**思路**：用 `tokio::select!` 将子 Agent 的 `execute()` 调用与 `child_cancel.cancelled()` 竞速。`child_cancel` 通过 `self.cancel.as_ref().map(|t| t.child_token())` 创建，链接到父 Agent 的取消令牌。

**代码**：
```rust
let child_cancel_for_select = child_cancel.clone();
let exec_fut = agent_builder.execute(..., Some(child_cancel));
let exec_result = tokio::select! {
    biased;
    _ = child_cancel_for_select.cancelled() => {
        Err(AgentError::Interrupted)
    }
    result = exec_fut => result,
};
```

**结果**：❌ 无效。SubAgent 继续执行到自然结束。

### 尝试 2：在 `executor::execute_prompt()` 添加 `tokio::select!`（纵深防御）

**文件**：`peri-acp/src/session/executor.rs:468`

**思路**：在更高层级（父 Agent 的 `execute()` 调用处）加第二层 `select!`。

**代码**：
```rust
let cancel_for_select = cancel.clone();
let mut executor = agent_output.executor;
let exec_fut = executor.execute(..., Some(cancel.clone()));
let result = tokio::select! {
    biased;
    _ = cancel_for_select.cancelled() => {
        Err(AgentError::Interrupted)
    }
    result = exec_fut => result,
};
```

**结果**：❌ 无效。与尝试 1 叠加后仍无法中断。

## 取消令牌传播链路（文档记录）

理论上 Ctrl+C 的取消信号应沿以下路径传播：

```
Ctrl+C → App::interrupt()
       → acp_client.cancel()                    [发送 $/cancel_request 通知]
       → handle_notification()                  [接收通知]
       → state.cancel_token.cancel()            [取消会话级令牌]
       → cancel (executor::execute_prompt)     [同一令牌的 clone]
       → build_agent(cancel.clone())           [传给中间件构建]
       → SubAgentMiddleware::with_cancel()     [传给子 Agent 中间件]
       → SubAgentTool::with_cancel()           [传给工具实例]
       → self.cancel.child_token()             [创建子令牌]
       → child_cancel (子 Agent execute)       [传入子 Agent 执行循环]
```

## 现状

- 会话级 `cancel_token` 确实被创建并存储（`prompt.rs:63-69`）
- `$/cancel_request` 通知通过 MPS 通道正常抵达服务端（`mod.rs:200-203`）
- 取消令牌的 `child_token()` 机制按 `tokio_util` 文档应能级联取消
- 但实际运行中，子 Agent 的取消检查点（`llm_step.rs:37`、`tool_dispatch.rs:234`）未触发，`select!` 包装也未生效
- 已添加的诊断日志尚未实际运行验证（已随代码回退移除）

## 下一步方向

1. **运行诊断日志**——在令牌创建、取消、竞速点添加 `tracing::info!` 日志，确认信号在链路的哪一步丢失
2. **检查 `active_agents` 注册**——`register_runtime()` 将 `child_cancel` 以 `"cascade"` 策略注册到 `active_agents` map（`define.rs:1134-1138`），可能影响级联取消的实际触发机制
3. **考虑替代方案**——如果 `CancellationToken::child_token()` 的级联行为在嵌套场景下不可靠，可考虑直接传递父令牌而非创建子令牌
