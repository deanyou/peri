# Compact 后渲染异常修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 compact（auto/manual）后 TUI 消息区域的三个渲染异常：loading spinner 不转、文字拖选蓝色底消失、拖选复制到错误文本。

**Architecture:** 根因分两层——(1) `compact_manual` 标志对所有 compact 都设 `true`，导致 auto-compact 后 `set_loading(false)` 被错误调用，spinner 永久丢失；(2) compact 后 `render_cache` 完全重建（`RebuildAll { prefix_len: 0 }`），`text_selection` 状态未同步清理，导致旧 visual 坐标与新 `wrap_map` 错位。修复策略是区分 auto/manual compact 的标志管理，并在 compact 重建时同步清理选区状态。

**Tech Stack:** Rust, ratatui, parking_lot::RwLock (RenderCache), mpsc channel (render thread)

**Issue:** `spec/issues/2026-06-07-compact-breaks-rendering-selection-loading.md`

---

### Task 1: 修复 `compact_manual` 标志误设（Loading 根因）

**Files:**
- Modify: `peri-tui/src/app/agent_compact.rs:5-18` (`handle_compact_started`)
- Modify: `peri-tui/src/app/agent_compact.rs:20-46` (`handle_compact_completed`)
- Test: `peri-tui/src/ui/headless_test.rs`（追加 compact loading 状态测试）

**根因分析：**

`handle_compact_started:11` 无条件设置 `compact_manual = true`，但此函数对 auto-compact 和 manual compact 都会被调用（经由 `agent_ops/mod.rs` 的 `AgentEvent::CompactStarted` 分支）。然后 `handle_compact_completed:42-46` 检查 `compact_manual` 来决定是否 `set_loading(false)`，导致 auto-compact 后 loading 也被错误清除。auto-compact 后 ReAct 循环内部继续执行，不会经过新的 `submit_message()`，所以 `set_loading(true)` 不会再被调用，spinner 永久丢失。

- [ ] **Step 1: 修改 `handle_compact_started`，将 `compact_manual` 参数化**

```rust
// peri-tui/src/app/agent_compact.rs

pub(crate) fn handle_compact_started(&mut self) -> (bool, bool, bool) {
    // 退出聚焦模式（如有）
    self.session_mgr.current_mut().focused_instance_id = None;
    self.session_mgr.current_mut().ui.bg_bar_cursor = None;

    // 🔴 删除: 不再无条件设 compact_manual = true
    // 改为在 manual compact 的调用点设置
    // self.session_mgr.current_mut().agent.compact_manual = true;

    // 显示 loading 状态（spinner + 禁用输入）
    self.set_loading(true);
    let vm = MessageViewModel::system(self.services.lc.tr("app-compact-started"));
    self.apply_pipeline_action(PipelineAction::AddMessage(vm));
    (true, false, false)
}
```

- [ ] **Step 2: 在 manual compact 的调用点设置 `compact_manual = true`**

找到 manual compact 的唯一调用路径（`/compact` 命令），在调用 `handle_compact_started` 之前设置标志。

搜索 `handle_compact_started` 的调用者：

```bash
grep -rn "handle_compact_started" peri-tui/src/
```

预期在 `agent_ops/mod.rs` 或 slash command handler 中找到调用点。manual compact 路径应在调用前设置：
```rust
self.session_mgr.current_mut().agent.compact_manual = true;
```

auto-compact 路径（`CompactMiddleware::before_model` 触发的）不设置此标志。

- [ ] **Step 3: 验证 `handle_compact_completed` 逻辑不变**

`handle_compact_completed:42-46` 的逻辑保持不变，现在 `compact_manual` 只在 manual compact 时为 `true`：
```rust
let is_manual = self.session_mgr.current_mut().agent.compact_manual;
if is_manual {
    self.set_loading(false);  // 仅 manual compact 结束 loading
    self.session_mgr.current_mut().agent.compact_manual = false;
}
```

auto-compact 时 `compact_manual = false`，不会调用 `set_loading(false)`，loading 保持到 agent Done 事件。

- [ ] **Step 4: 在 headless_test.rs 追加 auto-compact loading 保持测试**

```rust
/// 验证 auto-compact 后 loading 状态保持（compact_manual 不误设）
#[tokio::test]
async fn test_auto_compact_preserves_loading() {
    let (mut app, _handle) = App::new_headless(80, 24).await;

    // 模拟 auto-compact started（不设 compact_manual）
    let (consume, _, _) = app.handle_compact_started();
    assert!(consume);
    assert!(app.session_mgr.current().ui.loading);
    assert!(!app.session_mgr.current().agent.compact_manual);

    // 模拟 auto-compact completed
    let msgs = vec![BaseMessage::new_human("summary")];
    let (consume, _, _) = app.handle_compact_completed(
        "summary".into(),
        vec![],
        vec![],
        0,
        msgs,
    );
    assert!(consume);
    // auto-compact 后 loading 应保持（不调用 set_loading(false)）
    assert!(app.session_mgr.current().ui.loading);
}

/// 验证 manual compact 后 loading 结束
#[tokio::test]
async fn test_manual_compact_ends_loading() {
    let (mut app, _handle) = App::new_headless(80, 24).await;

    // 模拟 manual compact（设 compact_manual）
    app.session_mgr.current_mut().agent.compact_manual = true;
    let (consume, _, _) = app.handle_compact_started();
    assert!(app.session_mgr.current().ui.loading);

    let msgs = vec![BaseMessage::new_human("summary")];
    let (consume, _, _) = app.handle_compact_completed(
        "summary".into(),
        vec![],
        vec![],
        0,
        msgs,
    );
    // manual compact 后 loading 应结束
    assert!(!app.session_mgr.current().ui.loading);
}
```

- [ ] **Step 5: 运行测试**

```bash
cargo test -p peri-tui --lib -- test_auto_compact_preserves_loading test_manual_compact_ends_loading
```

Expected: 两个测试都 PASS

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/app/agent_compact.rs peri-tui/src/ui/headless_test.rs
git commit -m "fix: auto-compact 后 loading 不再被错误清除

compact_manual 标志改为只在 manual compact(/compact 命令)时设置，
auto-compact 不设此标志，避免 handle_compact_completed 误判为 manual
导致 set_loading(false)。

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 2: Compact 后清理 text_selection 状态

**Files:**
- Modify: `peri-tui/src/app/agent_compact.rs:20-85` (`handle_compact_completed`)
- Test: `peri-tui/src/ui/headless_test.rs`（追加 text_selection clear 测试）

**根因分析：**

compact 后 `RebuildAll { prefix_len: 0 }` 完全替换了 `view_messages` 和 `render_cache`，`wrap_map` 中的 `visual_row` 编号重新从 0 开始。但 `text_selection` 的 `start/end` 仍保留着 compact 前的 visual 坐标，这些坐标在新 `wrap_map` 中指向完全不同的内容——导致：
1. 蓝色底高亮涂在错误位置（或 `visual_to_logical` 返回 None 导致不显示）
2. `extract_selected_text` 提取到错误行的 `plain_text`

修复方案：在 `handle_compact_completed` 的 `RebuildAll` 之前清理 `text_selection`。

- [ ] **Step 1: 在 `handle_compact_completed` 中清理 text_selection**

在 `handle_compact_completed` 函数开头（`micro_cleared` 检查之后），添加 `text_selection.clear()`：

```rust
// peri-tui/src/app/agent_compact.rs — handle_compact_completed

// Full compact: 清理 pipeline + 更新内部状态
// 清理 text_selection：compact 后 wrap_map 完全重建，
// 旧的 visual 坐标在新 wrap_map 中无效
self.session_mgr.current_mut().ui.text_selection.clear();
```

插入位置：在 `let is_manual = ...` 行之前。

- [ ] **Step 2: 同时在 `handle_compact_started` 中也清理 text_selection**

compact 开始时也应清理，因为 compact 过程中消息区域会被替换：

```rust
// peri-tui/src/app/agent_compact.rs — handle_compact_started

// 清理 text_selection：compact 将重建所有消息
self.session_mgr.current_mut().ui.text_selection.clear();
```

- [ ] **Step 3: 追加测试**

```rust
/// 验证 compact 后 text_selection 被清理
#[tokio::test]
async fn test_compact_clears_text_selection() {
    let (mut app, _handle) = App::new_headless(80, 24).await;

    // 模拟用户有活跃的 text_selection
    app.session_mgr.current_mut().ui.text_selection.start_drag(50, 10);
    app.session_mgr.current_mut().ui.text_selection.update_drag(60, 20);
    assert!(app.session_mgr.current_mut().ui.text_selection.is_active());

    // compact started 应清理选区
    app.handle_compact_started();
    assert!(!app.session_mgr.current().ui.text_selection.is_active(),
        "text_selection 应在 compact_started 时被清理");

    // 再次设置选区
    app.session_mgr.current_mut().ui.text_selection.start_drag(5, 3);
    assert!(app.session_mgr.current_mut().ui.text_selection.is_active());

    // compact completed 也应清理选区
    let msgs = vec![BaseMessage::new_human("summary")];
    app.handle_compact_completed("summary".into(), vec![], vec![], 0, msgs);
    assert!(!app.session_mgr.current().ui.text_selection.is_active(),
        "text_selection 应在 compact_completed 时被清理");
}
```

- [ ] **Step 4: 运行测试**

```bash
cargo test -p peri-tui --lib -- test_compact_clears_text_selection
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/agent_compact.rs peri-tui/src/ui/headless_test.rs
git commit -m "fix: compact 后清理 text_selection 防止坐标错位

compact 的 RebuildAll 完全替换 view_messages 和 render_cache，
wrap_map 的 visual_row 编号重新从 0 开始。旧的 text_selection
坐标在新 wrap_map 中指向错误内容，导致蓝色底消失和复制错误文本。
在 compact_started 和 compact_completed 时均清理选区。

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 3: 验证修复——构建 + 测试 + 手动测试

**Files:**
- 无新文件

- [ ] **Step 1: 全量构建**

```bash
cargo build -p peri-tui
```

Expected: 编译成功，无 warning

- [ ] **Step 2: 全量测试**

```bash
cargo test -p peri-tui --lib
```

Expected: 所有测试 PASS

- [ ] **Step 3: 手动测试 checklist**

启动 TUI，执行以下场景：

| 场景 | 操作 | 预期结果 |
|------|------|---------|
| Auto-compact loading | 对话到触发 auto-compact | compact 后 spinner 继续转动，直到 agent 完成 |
| Manual compact loading | 执行 `/compact` | compact 后 spinner 停止，发送新消息后 spinner 恢复 |
| 文字拖选 | compact 后拖选文字 | 蓝色高亮正常显示，复制内容正确 |
| 长对话 | 多次 compact 后持续对话 | 所有渲染反馈正常 |

- [ ] **Step 4: 最终 Commit（如有调整）**

```bash
git add -A
git commit -m "chore: compact 渲染修复后的小调整

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 4: 更新 Issue 文档

**Files:**
- Modify: `spec/issues/2026-06-07-compact-breaks-rendering-selection-loading.md`

- [ ] **Step 1: 更新 issue 状态为 Fixed**

在 issue 文档底部追加修复记录：

```markdown
## 修复记录

| 日期 | 修复内容 | Commit |
|------|---------|--------|
| 2026-06-07 | Task 1: compact_manual 标志只在 manual compact 时设置，修复 auto-compact 后 loading 丢失 | `abc1234` |
| 2026-06-07 | Task 2: compact_started/completed 时清理 text_selection，修复蓝色底消失和复制错误 | `def5678` |
```

- [ ] **Step 2: Commit**

```bash
git add spec/issues/2026-06-07-compact-breaks-rendering-selection-loading.md
git commit -m "docs: 更新 compact 渲染 issue 修复记录

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```
