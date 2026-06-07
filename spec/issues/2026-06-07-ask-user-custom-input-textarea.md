# AskUser 弹窗自定义输入行替换为 textarea 组件

**状态**：Open
**优先级**：高
**创建日期**：2026-06-07

## 问题描述

AskUser 弹窗的自定义输入行使用 `String + usize cursor` 手写的单行编辑实现（`handle_edit_key()`），功能严重受限：空格键 bug（已修但仍暴露手写输入的脆弱性）、无法粘贴多行文本、无法输入长文本。应替换为主输入框同款的 `tui_textarea::TextArea` 组件，获得完整编辑能力。

## 症状详情

| 问题 | 表现 | 根因 |
|------|------|------|
| 空格键 bug | 自定义输入行无法输入空格 | `handle_edit_key` 是手写状态机，空格键在 match 中被 toggle 分支截获 |
| 无法粘贴 | 粘贴多行文本时丢失内容或异常 | `handle_edit_key` 只处理单字符插入，无 Paste 事件处理 |
| 无法输入长文本 | 只能单行输入，没有自动换行 | `String + usize cursor` 模型只支持单行 |
| 编辑能力弱 | 无选中、无 Ctrl+W 删词等 | 手写编辑器能力有限 |

## 期望行为

- 自定义输入行替换为 `tui_textarea::TextArea` 组件
- 初始 1 行高度，随内容自动扩展到最多 5 行
- 支持完整编辑能力：粘贴、多行、选中、删词等（与主输入框一致）
- 选项列表保持不变，textarea 仅替换最后一个编号选项

## 涉及文件

- `peri-tui/src/app/ask_user_prompt.rs` —— `QuestionState`：`custom_input: String` + `custom_cursor: usize` 替换为 `TextArea`，`in_custom_input` 状态保留
- `peri-tui/src/ui/main_ui/popups/ask_user.rs` —— `render_ask_user_popup`：自定义输入行渲染改为 `f.render_widget(textarea, area)`，高度随 textarea 行数动态计算
- `peri-tui/src/app/ask_user_ops.rs` —— `ask_user_edit_key`：从 `handle_edit_key()` 改为 `textarea.input(input)`
- `peri-tui/src/event/keyboard/popups.rs` —— Space 键和字符输入的 match arm：直接委托给 textarea 的 input 方法
- `peri-tui/src/app/edit_utils.rs` —— `build_textarea` 函数参考，可能需要新增一个针对 AskUser 场景的 textarea 构建函数

## 技术约束

- textarea 必须是 `TextArea<'static>`（与主输入框一致），不能持有引用
- 弹窗高度计算（`active_panel_height`）需考虑 textarea 的动态行数（1-5 行）
- Ctrl+U/Ctrl+D 已绑定为弹窗翻页，不能被 textarea 截获；textarea 内的 Ctrl+U（删到行首）需要区分处理
- Tab 键已绑定为切换问题 tab，textarea 内不能被 textarea 截获

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |

## 修复记录

（待 fix-issue 或 issue-verify skill 追加）
