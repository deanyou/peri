# Rewind 回填用户输入 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewind 撤回消息后，将被撤回的用户消息文本自动填入输入框，用户可直接编辑后重新发送。

**Architecture:** 在 `rewind_confirm()` 同步阶段从 `origin_messages` 提取被撤回消息的完整文本，暂存到 `UiState.pending_rewind_text`；在 `handle_rewind_completed()` 异步回调阶段消费该暂存值，通过 `textarea.insert_str()` 回填到输入框。全程仅在 TUI 层修改，不涉及 `peri-agent`/`peri-acp` 事件定义变更。

**Tech Stack:** Rust, tui-textarea-2, 已有的 `build_textarea()` + `insert_str()` 模式

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `peri-tui/src/app/ui_state.rs` | Modify | 添加 `pending_rewind_text: Option<String>` 字段 |
| `peri-tui/src/app/agent_ops/rewind.rs` | Modify | `rewind_confirm()` 提取完整文本并暂存 |
| `peri-tui/src/app/agent_compact.rs` | Modify | `handle_rewind_completed()` 消费暂存值回填输入框 |

---

### Task 1: UiState 添加暂存字段

**Files:**
- Modify: `peri-tui/src/app/ui_state.rs:49-50`（在 `diff_visible` 之后添加字段）
- Modify: `peri-tui/src/app/ui_state.rs:85`（在 `new()` 初始化块中添加字段初始化）

- [ ] **Step 1: 在 `UiState` struct 中添加字段**

在 `ui_state.rs` 的 `UiState` struct 中，`diff_visible` 字段之后添加：

```rust
    /// Rewind 完成后待回填到输入框的用户消息文本
    pub pending_rewind_text: Option<String>,
```

- [ ] **Step 2: 在 `UiState::new()` 中初始化该字段**

在 `ui_state.rs` 的 `UiState::new()` 初始化块中，`diff_visible: diff_enabled,` 之后添加：

```rust
            pending_rewind_text: None,
```

- [ ] **Step 3: 验证编译通过**

Run: `cargo build -p peri-tui 2>&1 | tail -5`
Expected: 编译成功（新字段为 `Option`，所有构造点通过 `UiState::new()` 初始化，无 break）

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/app/ui_state.rs
git commit -m "feat(rewind): add pending_rewind_text field to UiState"
```

---

### Task 2: rewind_confirm 提取并暂存完整文本

**Files:**
- Modify: `peri-tui/src/app/agent_ops/rewind.rs:111-148`（`rewind_confirm` 函数）

- [ ] **Step 1: 在 `rewind_confirm` 中提取被撤回消息的完整文本**

`rewind_confirm()` 中，在获取 `target_id` 的同一个闭包块里（第 112-124 行），同时查找 `origin_messages` 中目标消息的完整文本。

将 `rewind_confirm` 方法中的 `let (target_id, revert_files, go)` 闭包块替换为：

```rust
    pub(crate) fn rewind_confirm(&mut self) {
        let (target_id, revert_files, go, rewound_text) = {
            let session = self.session_mgr.current();
            if let Some(InteractionPrompt::Rewind(prompt)) = &session.agent.interaction_prompt {
                let item = &prompt.items[prompt.cursor];
                // 从 origin_messages 查找目标消息的完整文本（此时 origin_messages 尚未被 rewind 修改）
                let full_text = session
                    .agent
                    .origin_messages
                    .iter()
                    .find(|m| m.id().as_uuid().to_string() == item.message_id)
                    .map(|m| m.content().to_string());
                match prompt.mode {
                    RewindMode::MessagesAndFiles => {
                        (item.message_id.clone(), true, false, full_text)
                    }
                    RewindMode::ConfirmRevert => (item.message_id.clone(), true, true, full_text),
                    RewindMode::MessagesOnly => (item.message_id.clone(), false, true, full_text),
                }
            } else {
                return;
            }
        };
```

然后在 `rewind_confirm` 方法体中，`// 关闭弹窗` 行之后、`// 构造 /rewind 命令` 行之前，添加暂存逻辑：

```rust
        // 关闭弹窗
        self.session_mgr.current_mut().agent.interaction_prompt = None;

        // 暂存被撤回消息的文本，待 handle_rewind_completed 回填到输入框
        self.session_mgr.current_mut().ui.pending_rewind_text = rewound_text;

        // 构造 /rewind 命令并发送（复用 submit_message 的完整提交流程）
```

- [ ] **Step 2: 验证编译通过**

Run: `cargo build -p peri-tui 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/agent_ops/rewind.rs
git commit -m "feat(rewind): extract and stash rewound message text in rewind_confirm"
```

---

### Task 3: handle_rewind_completed 回填输入框

**Files:**
- Modify: `peri-tui/src/app/agent_compact.rs:103-137`（`handle_rewind_completed` 函数）

- [ ] **Step 1: 在 `handle_rewind_completed` 末尾消费暂存值回填输入框**

在 `agent_compact.rs` 的 `handle_rewind_completed` 方法中，在 `apply_pipeline_action(PipelineAction::RebuildAll { ... })` 之后、`return` 之前，添加回填逻辑：

将现有末尾：

```rust
        self.apply_pipeline_action(PipelineAction::RebuildAll {
            prefix_len: 0,
            tail_vms: view_msgs,
        });

        (true, false, false)
```

替换为：

```rust
        self.apply_pipeline_action(PipelineAction::RebuildAll {
            prefix_len: 0,
            tail_vms: view_msgs,
        });

        // 将被撤回的用户消息文本回填到输入框
        if let Some(text) = self.session_mgr.current_mut().ui.pending_rewind_text.take() {
            let textarea = &mut self.session_mgr.current_mut().ui.textarea;
            textarea.insert_str(&text);
        }

        (true, false, false)
```

注意：`insert_str` 是 `tui_textarea::TextArea` 的方法，直接在光标位置插入文本。rewind 后 textarea 已清空（`build_textarea` 在 `restore_history_to_textarea` 和 `restore_draft` 中调用，但 `handle_rewind_completed` 不调用这些——textarea 保持之前的空状态），所以 `insert_str` 会在 textarea 开头插入完整文本。

- [ ] **Step 2: 验证编译通过**

Run: `cargo build -p peri-tui 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/agent_compact.rs
git commit -m "feat(rewind): restore rewound text to input textarea on rewind completion"
```

---

### Task 4: 全量构建 + Clippy 检查

**Files:** 无修改

- [ ] **Step 1: 运行全量构建**

Run: `cargo build 2>&1 | tail -10`
Expected: 所有 crate 编译成功

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy -p peri-tui 2>&1 | tail -10`
Expected: 无 warning/error

- [ ] **Step 3: 最终 commit（如有 lint 修复）**

仅在 Step 1/2 发现问题时执行修复并提交。

---

## Self-Review

**1. Spec coverage:**
- ✅ "rewind 后文本回填输入框" → Task 2（暂存）+ Task 3（回填）
- ✅ "可编辑后重新发送" → `insert_str` 将文本放入 textarea，用户自然可编辑
- ✅ "复用现有机制" → 使用 `insert_str()` 模式（与 `history_ops.rs:86` 一致）

**2. Placeholder scan:**
- 无 TBD/TODO/填空
- 每步有完整代码

**3. Type consistency:**
- `pending_rewind_text: Option<String>` — 定义在 `UiState`，通过 `.take()` 在 `handle_rewind_completed` 中消费（返回 `Option<String>`，`if let Some(text)` 解构）
- `rewound_text` 类型为 `Option<String>` — 在闭包中通过 `.map(|m| m.content().to_string())` 得到
- `m.content()` 返回 `&str`，`.to_string()` 转为 `String` — 一致

**4. 边界情况考虑:**
- `origin_messages` 为空：`rewind_confirm` 不会执行到（`open_rewind_prompt` 已拦截）
- `message_id` 查找失败：`full_text = None`，`pending_rewind_text = None`，回填时 `if let Some` 跳过——优雅降级
- 多次 rewind：每次 `rewind_confirm` 覆写 `pending_rewind_text`，每次 `handle_rewind_completed` `.take()` 消费——无残留
