# AskUser 弹窗自定义输入无法键入空格

**状态**：Pending
**优先级**：中
**创建日期**：2026-06-07

## 问题描述

AskUser 弹窗（Questions 交互）的自定义输入行中，按空格键不会插入空格字符，而是触发了选项 toggle（虽然 toggle 因 `in_custom_input` 检查而 no-op）。用户无法在自定义输入中输入包含空格的文本。

## 症状详情

- 用户在 AskUser 弹窗中将光标移到最后一行（自定义输入行），按空格键无反应
- `Key::Char(' ')` 在 `popups.rs` 的 match 中先匹配到 `ask_user_toggle()` 分支，`toggle_current()` 检测到 `in_custom_input` 后 no-op 返回
- 空格键永远无法到达 `_ =>` fallback 分支中的 `ask_user_edit_key()`
- 表现为：自定义输入框中无法输入空格字符

**对比正确实现**：Config 面板（`config_panel.rs:420-441`）在 `Key::Char(' ')` 的 match arm 内区分 toggle 行和文本行，文本行调用 `input_char(' ')`；Setup Wizard（`ops.rs:247-266`）检查 `is_text_input()` 后调用 `handle_edit_key()`。

## 涉及文件

- `peri-tui/src/event/keyboard/popups.rs`（第 56-63 行）—— Space 键 match 逻辑，需在 toggle 前检查 `in_custom_input`
- `peri-tui/src/app/ask_user_ops.rs`（第 83-96 行）—— `ask_user_edit_key` 已正确委托 `handle_edit_key`，无需修改
- `peri-tui/src/app/ask_user_prompt.rs`（第 41-54 行）—— `toggle_current` 的 `in_custom_input` early return 逻辑

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |
| 2026-06-07 | Open | Pending | agent | 最小修复：Space 处理中同时调用 edit_key 和 toggle，利用 in_custom_input 互斥 |

## 修复记录

### 修复 #1（2026-06-07）

- **操作人**：agent
- **用户原意**：AskUser 弹窗自定义输入行应能输入空格字符
- **修复内容**：将 `popups.rs` 中 Space 键的 match arm 从仅调用 `ask_user_toggle()` 改为先调用 `ask_user_edit_key(input)` 再调用 `ask_user_toggle()`。两者通过 `in_custom_input` 检查互斥——custom input 模式下 edit_key 插入空格、toggle no-op；选项模式下 edit_key no-op、toggle 切换选项。
- **涉及文件**：`peri-tui/src/event/keyboard/popups.rs`
- **验证状态**：待验证
