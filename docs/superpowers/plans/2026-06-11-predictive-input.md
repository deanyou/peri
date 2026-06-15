# Predictive Input 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agent 完成后自动发起一次 LLM 请求预测用户下一步输入，以灰色 placeholder 显示在输入框，Tab 接受。

**Architecture:** ACP server 在 `execute_prompt()` 成功返回后 `tokio::spawn` 一个轻量 prediction 任务，构建 `ReActAgent(max_iterations=1, 无工具)` + prediction directive，结果通过 `peri/prediction_ready` 自定义通知发送到 TUI。TUI 接收后存入 `UiState.prediction`，渲染为 textarea 上的灰色叠加文本。

**Tech Stack:** Rust, tokio, ratatui, peri-agent ReActAgent, MpscTransport

---

## 文件结构

| 文件 | 变更 | 职责 |
|------|------|------|
| `peri-middlewares/src/subagent/fork.rs` | 修改 | 新增 `build_prediction_directive()` |
| `peri-tui/src/app/ui_state.rs` | 修改 | 新增 `PredictionState` + `prediction` 字段 |
| `peri-tui/src/acp_client/client.rs` | 修改 | 新增 `PredictionReady` 变体 + pump 解析 |
| `peri-tui/src/app/agent_ops/acp_bridge.rs` | 修改 | 处理 `PredictionReady` |
| `peri-tui/src/ui/main_ui/mod.rs` | 修改 | textarea placeholder 渲染 |
| `peri-tui/src/event/keyboard/normal_keys.rs` | 修改 | Tab 接受 + 输入清除 |
| `peri-tui/src/app/mod.rs` | 修改 | `set_loading(true)` 清除 prediction |
| `peri-tui/src/acp_server/prompt.rs` | 修改 | `execute_prompt()` 返回后 spawn prediction |

---

### Task 1: 新增 Prediction Directive 模板

**Files:**
- Modify: `peri-middlewares/src/subagent/fork.rs`

- [ ] **Step 1: 添加 `build_prediction_directive` 函数**

在 `fork.rs` 末尾添加：

```rust
/// 构建 Prediction 指令模板（中文）。
/// 用于 agent 完成后预测用户下一步输入。
pub fn build_prediction_directive() -> String {
    "<prediction_directive>\n\
     你是预测输入助手。根据对话上下文，预测用户下一步最可能在输入框中输入什么。\n\
     \n\
     规则：\n\
     1. 只输出一句预测文本，不要解释\n\
     2. 预测应该是自然的用户语言，像用户自己会打的那样\n\
     3. 不要加引号、前缀或格式\n\
     4. 长度控制在 5-30 个字\n\
     5. 如果无法判断，输出空字符串\n\
     </prediction_directive>".to_string()
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-middlewares`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/subagent/fork.rs
git commit -m "feat: add prediction directive template for predictive input"
```

---

### Task 2: TUI UiState 新增 PredictionState

**Files:**
- Modify: `peri-tui/src/app/ui_state.rs`

- [ ] **Step 1: 添加 `PredictionState` 结构体和 `prediction` 字段**

在 `use` 块之后、`UiState` 结构体之前添加：

```rust
/// 预测输入状态：agent 完成后 LLM 生成的下一步输入建议。
pub struct PredictionState {
    pub text: String,
    pub received_at: std::time::Instant,
}
```

在 `UiState` 结构体中，`pending_rewind_text` 字段之后添加：

```rust
    /// 预测输入建议（灰色 placeholder，Tab 接受）
    pub prediction: Option<PredictionState>,
```

在 `UiState::new()` 中，`pending_rewind_text: None,` 之后添加：

```rust
            prediction: None,
```

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功（可能有 unused warning，后续 task 会使用）

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/ui_state.rs
git commit -m "feat: add PredictionState to UiState"
```

---

### Task 3: ACP Notification 新增 PredictionReady

**Files:**
- Modify: `peri-tui/src/acp_client/client.rs`

- [ ] **Step 1: 新增 `PredictionReady` 变体到 `AcpNotification`**

在 `AcpNotification::Peri` 变体之前添加：

```rust
    /// Prediction fork 完成后的建议文本。
    PredictionReady { session_id: String, text: String },
```

- [ ] **Step 2: 在 `run_pump` 中解析 `peri/prediction_ready` 通知**

在 `run_pump()` 的 `else if method.starts_with("notifications/peri/")` 分支之前，添加：

```rust
                        } else if method == "peri/prediction_ready" {
                            let session_id = params
                                .get("sessionId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let text = params
                                .get("text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !text.is_empty() {
                                let _ = notification_tx
                                    .send(AcpNotification::PredictionReady { session_id, text });
                            }
```

- [ ] **Step 3: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功（PredictionReady 未使用，warning 正常）

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/acp_client/client.rs
git commit -m "feat: add PredictionReady AcpNotification and pump parsing"
```

---

### Task 4: TUI ACP Bridge 处理 PredictionReady

**Files:**
- Modify: `peri-tui/src/app/agent_ops/acp_bridge.rs`

- [ ] **Step 1: 在 `handle_acp_notification` 中处理 `PredictionReady`**

在 `AcpNotification::AgentDone` 分支之后、`AcpNotification::RequestPermission` 分支之前添加：

```rust
            AcpNotification::PredictionReady { text, .. } => {
                // textarea 为空时才显示 prediction（用户可能已开始输入）
                let textarea_empty = self
                    .session_mgr
                    .current_mut()
                    .ui
                    .textarea
                    .lines()
                    .iter()
                    .all(|l| l.is_empty());
                if textarea_empty && !self.session_mgr.current_mut().ui.loading {
                    self.session_mgr.current_mut().ui.prediction =
                        Some(crate::app::ui_state::PredictionState {
                            text,
                            received_at: std::time::Instant::now(),
                        });
                }
                (true, false, false)
            }
```

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/agent_ops/acp_bridge.rs
git commit -m "feat: handle PredictionReady in ACP bridge"
```

---

### Task 5: TUI 渲染 Placeholder 叠加

**Files:**
- Modify: `peri-tui/src/ui/main_ui/mod.rs`

- [ ] **Step 1: 在 textarea 渲染后叠加 placeholder 文本**

在 `app.session_mgr.current_mut().ui.textarea_area = Some(chunks[5]);` 之后，添加 placeholder 叠加渲染逻辑：

```rust
    // Prediction placeholder 叠加（textarea 为空 + 有 prediction 时显示）
    if let Some(ref pred) = app.session_mgr.current().ui.prediction {
        let textarea_empty = app
            .session_mgr
            .current()
            .ui
            .textarea
            .lines()
            .iter()
            .all(|l| l.is_empty());
        if textarea_empty {
            let area = chunks[5];
            // placeholder 跟 textarea 内容区域对齐（❯ prompt 占 2 列 + padding）
            let pred_area = ratatui::layout::Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: 1,
            };
            let pred_text = ratatui::text::Line::from(
                ratatui::text::Span::styled(&pred.text, ratatui::style::Style::default().fg(super::theme::DIM))
            );
            f.render_widget(
                ratatui::widgets::Paragraph::new(pred_text),
                pred_area,
            );
        }
    }
```

注意：`super::theme::DIM` 的路径需要根据实际 theme 导入位置调整。当前文件可能已导入 `use super::theme;` 或直接使用 `theme::DIM`。检查文件顶部的 `use` 语句确认。

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/ui/main_ui/mod.rs
git commit -m "feat: render prediction placeholder overlay on textarea"
```

---

### Task 6: Tab 接受 Prediction

**Files:**
- Modify: `peri-tui/src/event/keyboard/normal_keys.rs`

- [ ] **Step 1: 修改 `handle_tab` 函数，prediction 优先级最高**

修改 `handle_tab` 函数，在 `@mention` 检查之前插入 prediction 逻辑：

```rust
fn handle_tab(app: &mut App) {
    use super::inject_at_mention_path;

    // Prediction 接受优先级最高
    if let Some(pred) = app.session_mgr.current_mut().ui.prediction.take() {
        app.session_mgr.current_mut().ui.textarea.insert_str(&pred.text);
        return;
    }

    if app.session_mgr.current_mut().ui.at_mention.active {
        inject_at_mention_path(app);
    } else {
        // ... 原有 hint 逻辑不变 ...
    }
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/event/keyboard/normal_keys.rs
git commit -m "feat: Tab accepts prediction input"
```

---

### Task 7: 输入时清除 Prediction

**Files:**
- Modify: `peri-tui/src/event/keyboard/normal_keys.rs`
- Modify: `peri-tui/src/app/mod.rs`

- [ ] **Step 1: 在默认输入分支中清除 prediction**

在 `normal_keys.rs` 的 `input if input.key != Key::Enter =>` 分支中，`textarea.input(input)` 之后添加：

```rust
            app.session_mgr.current_mut().ui.textarea.input(input);
            // 任意输入清除 prediction
            app.session_mgr.current_mut().ui.prediction = None;
```

- [ ] **Step 2: 在 `set_loading(true)` 中清除 prediction**

在 `peri-tui/src/app/mod.rs` 的 `set_loading` 函数中，`s.ui.textarea = build_textarea(true);` 之前添加：

```rust
        s.ui.prediction = None;
```

- [ ] **Step 3: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/event/keyboard/normal_keys.rs peri-tui/src/app/mod.rs
git commit -m "feat: clear prediction on user input and new agent round"
```

---

### Task 8: ACP Server Spawn Prediction Task

**Files:**
- Modify: `peri-tui/src/acp_server/mod.rs`

这是核心集成点。在 `session/prompt` 的 tokio::spawn 内，`execute_prompt()` 返回后、发送 response 之前，spawn prediction task。

- [ ] **Step 1: 添加 prediction spawn 逻辑**

在 `peri-tui/src/acp_server/mod.rs` 的 `tokio::spawn` 块内，`let _ = transport.send_response(id, result).await;` **之前**添加 prediction spawn：

```rust
                        // Prediction: agent 成功完成后发起预测输入请求
                        let prompt_ok = result
                            .as_ref()
                            .map(|r| r.get("stopReason").and_then(|v| v.as_str()) == Some("end_turn"))
                            .unwrap_or(false);
                        if prompt_ok {
                            let pred_transport = Arc::clone(&transport);
                            let pred_session_id = prompt_session_id.clone();
                            let pred_peri_config = peri_config.clone();
                            let pred_sessions = sessions.clone();

                            tokio::spawn(async move {
                                // 从 session 获取最新历史
                                let (history, cwd) = {
                                    let sessions = pred_sessions.lock().await;
                                    match sessions.get(&pred_session_id) {
                                        Some(s) => (s.history.clone(), s.cwd.clone()),
                                        None => return,
                                    }
                                };

                                // 取最近 10 条消息作为上下文（排除 System 消息）
                                let recent: Vec<_> = history
                                    .iter()
                                    .rev()
                                    .filter(|m| !m.is_system())
                                    .take(10)
                                    .cloned()
                                    .collect();
                                let recent: Vec<_> = recent.into_iter().rev().collect();

                                if recent.is_empty() {
                                    return;
                                }

                                // 构造 LLM
                                let provider = {
                                    let cfg = pred_peri_config.read();
                                    match crate::app::agent::LlmProvider::from_config(&cfg) {
                                        Some(p) => p,
                                        None => return,
                                    }
                                };

                                let llm = peri_agent::llm::BaseModelReactLLM::new(provider.into_model());
                                let llm = peri_agent::llm::RetryableLLM::new(llm, peri_agent::llm::RetryConfig::default());

                                // 构建最小 agent（1 轮、无工具、无中间件）
                                let directive = peri_middlewares::subagent::fork::build_prediction_directive();
                                let mut agent = peri_agent::agent::executor::ReActAgent::new(llm)
                                    .max_iterations(1)
                                    .with_system_prompt(directive);

                                // 构造 state，注入对话历史
                                let mut state = peri_agent::agent::state::AgentState::new();
                                for msg in &recent {
                                    state.add_message(msg.clone());
                                }

                                let cancel_token = peri_agent::agent::AgentCancellationToken::new();

                                // 5 秒超时
                                let result = tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    agent.execute(
                                        peri_agent::agent::input::AgentInput::text("请根据以上对话预测用户下一步输入"),
                                        &mut state,
                                        cancel_token,
                                    ),
                                ).await;

                                match result {
                                    Ok(Ok(_output)) => {
                                        // 提取最后一条 AI 消息文本
                                        let text = state.messages()
                                            .iter()
                                            .rev()
                                            .find_map(|m| {
                                                if m.is_ai() {
                                                    m.content().text_content().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
                                                } else {
                                                    None
                                                }
                                            })
                                            .unwrap_or_default();

                                        if !text.is_empty() {
                                            let _ = pred_transport
                                                .send_notification(
                                                    "peri/prediction_ready",
                                                    serde_json::json!({
                                                        "sessionId": pred_session_id,
                                                        "text": text,
                                                    }),
                                                )
                                                .await;
                                        }
                                    }
                                    Ok(Err(e)) => {
                                        tracing::debug!(error = %e, "Prediction fork failed");
                                    }
                                    Err(_) => {
                                        tracing::debug!("Prediction fork timed out (5s)");
                                    }
                                }
                            });
                        }
```

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-tui`
Expected: 编译成功。注意可能需要调整 import 路径——检查 `peri_agent` 和 `peri_middlewares` 的 crate 可见性。peri-tui 依赖 peri-agent 和 peri-middlewares（类型依赖），所以这些类型应该可用。

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/acp_server/mod.rs
git commit -m "feat: spawn prediction task after successful agent completion"
```

---

### Task 9: 集成验证

- [ ] **Step 1: 全量编译**

Run: `cargo build`
Expected: 编译成功

- [ ] **Step 2: 运行现有测试**

Run: `cargo test -p peri-tui --lib`
Expected: 所有测试通过

Run: `cargo test -p peri-middlewares --lib`
Expected: 所有测试通过

- [ ] **Step 3: 手动冒烟测试**

1. 启动 TUI：`cargo run -p peri-tui`
2. 输入任意 prompt，等待 agent 完成响应
3. 验证：输入框出现灰色 placeholder 文字
4. 按 Tab → 验证 placeholder 文本填入输入框
5. 不按 Tab，直接开始输入 → 验证 placeholder 消失
6. 发起新一轮对话 → 验证 prediction 在 agent 完成后重新出现

- [ ] **Step 4: 最终 Commit**

```bash
git add -A
git commit -m "feat: predictive input — agent-completion prediction with Tab accept"
```

---

## 自检

**Spec 覆盖检查：**

| Spec 要求 | Task |
|-----------|------|
| Prediction 指令模板 | Task 1 |
| PredictionState + UiState | Task 2 |
| AcpNotification::PredictionReady | Task 3 |
| TUI pump 解析 | Task 3 |
| ACP bridge 处理 | Task 4 |
| Placeholder 渲染 | Task 5 |
| Tab 接受 | Task 6 |
| 输入取消 | Task 7 |
| set_loading 清除 | Task 7 |
| ACP server spawn | Task 8 |
| 5 秒超时 | Task 8 |
| 空 textarea 才显示 | Task 4 |
| 取消时机（新 agent 轮次） | Task 7 |
| 集成验证 | Task 9 |

**Placeholder 扫描：** 无 TBD/TODO。所有代码步骤包含完整实现。

**类型一致性检查：**
- `PredictionState` 在 `ui_state.rs` 定义 → `acp_bridge.rs` 和 `normal_keys.rs` 使用 `crate::app::ui_state::PredictionState` ✓
- `AcpNotification::PredictionReady { session_id, text }` 在 `client.rs` 定义 → `acp_bridge.rs` match ✓
- `build_prediction_directive()` 在 `fork.rs` 定义 → `mod.rs` 通过 `peri_middlewares::subagent::fork::build_prediction_directive()` 调用 ✓
- `AgentState::new()` / `add_message()` / `messages()` / `AgentInput::text()` — 需确认 peri-agent 公开 API。如不可用，改用 `state.add_message(BaseMessage::...)` 手动构建。

**潜在风险：**
1. `peri_agent::agent::state::AgentState` 和 `peri_agent::agent::input::AgentInput` 的公开可见性——如果编译失败，需检查 `peri-agent` 的 pub 导出。
2. `ReActAgent::execute()` 返回的 `AgentOutput` 类型——Task 8 中忽略 output，只从 state 提取文本，这是安全的。
3. `textarea.lines()` 返回 `&[String]`——`all(|l| l.is_empty())` 检查空 textarea 正确。
4. `theme::DIM` 在渲染文件中的路径——需确认 `use super::theme` 或 `crate::ui::theme`。
