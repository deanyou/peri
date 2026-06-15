# Agent 工具调用在信息流中位置偏移

**状态**：Open
**优先级**：中
**创建日期**：2026-06-09

## 问题描述

Agent 执行时的工具调用（ToolBlock/ToolCallGroup）在信息流中的渲染位置出现偏移。工具调用卡片本应在消息列表的正确时间线位置显示，但现在它们的垂直位置不对。

用户描述为"被固定到 loading spinner 上面了，不是在信息流上面固定"，进一步澄清为"在信息流中偏移"——即工具调用内容在消息流中出现在了错误的垂直位置。

## 症状详情

| 场景 | 预期 | 实际 |
|------|------|------|
| Agent 正常执行并调用工具 | 工具调用卡片按时间顺序嵌入消息流中 | 工具调用内容在信息流中出现位置偏移 |

## 复现条件

- **复现频率**：需要确认
- **触发步骤**：Agent 发起任意工具调用，观察工具调用卡片在消息流中的位置
- **环境**：macOS，任意模型

## 涉及文件

- `peri-tui/src/ui/message_view/mod.rs` —— MessageViewModel 及 ToolBlock/ToolCallGroup 变体定义
- `peri-tui/src/ui/message_view/tools.rs` —— 工具调用相关的 ViewModel 构造
- `peri-tui/src/ui/message_view/aggregate.rs` —— ToolCallGroup 的聚合逻辑（影响工具调用在信息流中的位置）
- `peri-tui/src/app/message_pipeline/mod.rs` —— 消息管道中的 pending_tools 处理
- `peri-tui/src/app/message_pipeline/reconcile.rs` —— reconcile 中追加 pending tool blocks 的逻辑
- `peri-tui/src/app/message_pipeline/transform.rs` —— BaseMessage → MessageViewModel 转换
- `peri-tui/src/ui/main_ui/message_area.rs` —— render_messages 中 spinner 区域与消息内容的布局
- `peri-tui/src/ui/main_ui/sticky_header.rs` —— sticky header 功能（可能与工具调用固定逻辑相关）

## 关联 Issue

（无直接关联，为独立问题）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-09 | — | Open | agent | 创建 |

## 修复记录

（待 fix-issue 或 issue-verify skill 追加）
