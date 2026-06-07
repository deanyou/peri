# Compact 后消息区域渲染异常：文字拖选蓝色底消失 + Loading 状态丢失

**状态**：Pending
**优先级**：高
**创建日期**：2026-06-07
**修复日期**：2026-06-07

## 问题描述

执行 compact（手动 `/compact` 或自动 compact）后，TUI 消息区域出现两个独立但同时出现的渲染异常：
1. **文字拖选蓝色底消失**：鼠标拖选文字时不再显示蓝色高亮背景（`SELECTION_BG`），永久丢失
2. **Loading 状态丢失**：auto-compact 后 status bar 的 spinner 不再转动，agent 实际仍在工作

消息内容渲染正常，交互功能（点击、展开等操作）正常。问题在整个 session 生命周期内不可恢复。

## 症状详情

| 现象 | compact 前 | compact 后 |
|------|-----------|-----------|
| 文字拖选蓝色高亮背景 | 正常显示（SELECTION_BG） | 永久消失 |
| Status bar loading spinner | agent 执行时显示 | auto-compact 后不转 |
| 交互功能（点击/展开） | 正常 | 仍可正常操作 |
| 消息内容本身 | 正常渲染 | 正常渲染 |

### 现象 1：文字拖选蓝色底消失 + 复制内容错误

compact 后，鼠标拖选消息区域文字时不再显示蓝色高亮背景。拖选操作本身正常（松手后可以复制文字），但视觉反馈（`theme::SELECTION_BG` 蓝色底色）不显示。手动和自动 compact 后都会出现，不可恢复。

此外，拖选复制时提取到的文本内容**不是消息区域的文字，而是来自根本没有打开的 panel 的文本**。这表明 compact 后 `render_cache.wrap_map` 的坐标映射与 `text_selection` 的 visual 坐标完全错位——`extract_selected_text()` 用旧的 visual_row 坐标在新（极短的）wrap_map 中查找，定位到了错误的条目，提取到了不相关的文本。

### 现象 2：Auto-compact 后 Loading 状态丢失

auto-compact（上下文超阈值自动触发）完成后，status bar 的 spinner 停止转动。agent 实际仍在工作（能看到输出内容变化），但缺少 loading spinner 状态指示。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI，进行若干轮对话产生足够上下文
  2. 等待自动 compact 触发（或手动 `/compact`）
  3. compact 完成后尝试拖选文字 / 观察 spinner 状态
- **恢复条件**：不可恢复，整个 session 一直异常，直到新建 session 或重启 TUI

## 根因分析

### Loading 丢失根因（已确认）

**文件**：`peri-tui/src/app/agent_compact.rs:11`

`handle_compact_started()` 无条件设置 `compact_manual = true`：

```rust
pub(crate) fn handle_compact_started(&mut self) -> (bool, bool, bool) {
    // ...
    self.session_mgr.current_mut().agent.compact_manual = true;  // ← 对所有 compact 都设 true
    self.set_loading(true);
    // ...
}
```

但此函数对 auto-compact 和 manual compact **都会被调用**（经由 `agent_ops/mod.rs:281` 的 `AgentEvent::CompactStarted` 分支）。

然后在 `handle_compact_completed()` 中：

```rust
let is_manual = self.session_mgr.current_mut().agent.compact_manual;
if is_manual {
    self.set_loading(false);  // ← auto-compact 也被当成 manual，错误清除了 loading
}
```

**影响链**：auto-compact → `CompactStarted` → `compact_manual=true` → `CompactCompleted` → `set_loading(false)` → **loading 永久丢失**（因为 ReAct 循环内 compact 后不需要经过新的 submit_message，没有机会重新 `set_loading(true)`）

### 文字拖选蓝色底消失（待深入调查）

可能原因方向：
1. compact 后 `RebuildAll { prefix_len: 0 }` 完全替换了 view_messages 和 RenderCache，`wrap_map` 中的 `visual_row` 编号与 `text_selection` 存储的 visual 坐标不对齐
2. compact 触发的 `set_loading(true)` 重建 textarea 时清除了某些 UI 状态
3. RenderCache 重建后 `viewport_clip` 阶段 3 的选区高亮计算依赖的 `first_idx` 或 `wrap_map` 值不正确

## 涉及文件

- `peri-tui/src/app/agent_compact.rs` — compact 生命周期处理，`compact_manual` 标志误设的根因所在
- `peri-tui/src/ui/main_ui/message_area.rs` — 消息区域渲染，`viewport_clip` 阶段 3 的文字选区高亮逻辑
- `peri-tui/src/app/text_selection.rs` — `TextSelection` 状态，`visual_to_logical` 坐标转换
- `peri-tui/src/ui/render_thread.rs` — RenderCache 重建，`wrap_map` 生成

## 关联 Issue

- `spec/archive-issues/2026-05-25-compact-resubmit-missing-loading-spinner.md` — compact resubmit 时 loading spinner 缺失（已归档，相同根因——compact 后 loading 状态管理不当）
- `spec/archive-issues/2026-05-26-manual-compact-long-loading-skeleton.md` — 手动 compact loading 不消失（相反方向问题，当时引入 `compact_manual` 标志的修复）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |
| 2026-06-07 | Open | Fixed | agent | 修复 compact_manual + text_selection |
| 2026-06-07 | Fixed | Pending | agent | 等待用户手动验证（auto-compact spinner + 文字拖选蓝色底 + 复制内容） |

## 修复记录

| 日期 | 修复内容 | Commit |
|------|---------|--------|
| 2026-06-07 | 移除 `handle_compact_started` 中的 `compact_manual=true`，删除 `handle_compact_completed` 中的 `compact_manual` 检查和 `set_loading(false)`。Loading 统一由 `Done` 事件结束（manual compact 是 `CommandKind::Immediate`，executor 执行后调用 `push_done()`）。 | `88a200c8` |
| 2026-06-07 | 在 `handle_compact_started` 和 `handle_compact_completed` 中添加 `text_selection.clear()`，防止 `RebuildAll` 后旧 visual 坐标与新 `wrap_map` 错位。 | `88a200c8` |
| 2026-06-07 | 追加 2 个回归测试：`test_compact_completed_preserves_loading`、`test_compact_clears_text_selection`。 | `88a200c8` |
