# 实施计划：/clear 通过 ACP 清空上下文

**关联 Issue**：`spec/issues/2026-05-21-clear-command-doesnt-clear-live-context.md`

## 方案概述

`/clear` → `new_thread()` 后，通过 `AcpTuiClient` 发送 `session/clear` 请求给 ACP Server，Server 端在 `handle_request()` 中清空 `SessionState.history`。选用请求（request），与 `session/compact` 同级，有响应确认。

## 涉及文件（3 个）

| 文件 | 变更 |
|------|------|
| `peri-tui/src/acp_server/requests.rs` | `handle_request()` 新增 `"session/clear"` 分支 |
| `peri-tui/src/acp_client/client.rs` | 新增 `clear()` 方法（参照 `compact()`） |
| `peri-tui/src/app/thread_ops.rs` | `new_thread()` 末尾调用 `acp_client.clear()` |

## 详细步骤

### Step 1：ACP Server 端新增 `session/clear` 处理

**`peri-tui/src/acp_server/requests.rs`**（`handle_request()` 函数内，`session/compact` 后面）：

```rust
"session/clear" => {
    let session_id = extract_session_id(params, "");
    if let Some(state) = sessions.get_mut(session_id) {
        state.history.clear();
        info!(session_id = %session_id, "Session history cleared");
    }
    serde_json::to_value(serde_json::json!({ "ok": true }))
        .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
}
```

只需一行 `state.history.clear()`，不需要 spawn——无 LLM 调用。

### Step 2：AcpTuiClient 新增 `clear()` 方法

**`peri-tui/src/acp_client/client.rs`**：

参照 `compact()` 实现：

```rust
/// Clear conversation history on the ACP server.
pub async fn clear(&self) -> Result<(), String> {
    let session_id = self
        .current_session_id
        .lock()
        .unwrap()
        .clone()
        .ok_or("no active session")?;
    let params = json!({ "sessionId": session_id });
    self.transport
        .send_request("session/clear", params)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
```

### Step 3：`new_thread()` 调用 `clear()`

**`peri-tui/src/app/thread_ops.rs`**：

在 `new_thread()` 末尾（清除 TUI 状态之后）追加：

```rust
// 通知 ACP Server 清空会话历史
if let Some(ref acp_client) = self.acp_client {
    let client = acp_client.clone();
    tokio::spawn(async move {
        if let Err(e) = client.clear().await {
            tracing::warn!(error = %e, "Failed to clear ACP session history");
        }
    });
}
```

### Step 4：验证

1. 发送消息 → ACP Server `history` 不为空
2. 执行 `/clear` → ACP Server `history` 清空
3. 再发送消息 → LLM 看不到旧内容

## 不涉及的部分

- **ThreadStore 持久化**：`history` 是内存状态
- **`agent_state_messages`**：TUI 层已处理
- **Thread ID**：保持不变

## 风险

- **无竞态**：`handle_request` 在持有 `sessions` 锁时同步执行，`history.clear()` 立即生效
- **正在运行的 prompt 不受影响**：prompt 在 spawn 前已 clone `history`，clear 不影响已运行的调用
