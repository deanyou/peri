# 手动 /compact 后聊天区域长时间显示 loading 骨架屏

**状态**：Open
**优先级**：高
**创建日期**：2026-05-26

## 问题描述

用户在输入框执行 `/compact` 命令后，聊天区域显示 loading 骨架屏，持续 30 秒以上才恢复。恢复后内容正常显示、可继续对话。该问题每次手动 compact 必现，严重影响用户体验——compact 期间界面处于不可用状态，用户无法查看任何聊天内容。

## 症状详情

| 维度 | 表现 |
|------|------|
| 触发方式 | 在输入框输入 `/compact` 回车 |
| 触发后表现 | 聊天区域显示 loading 骨架屏（非 status bar spinner） |
| 持续时间 | 30 秒以上 |
| 恢复行为 | 最终自动恢复，恢复后 compact 摘要内容正常显示 |
| 恢复后状态 | 可继续对话，无内容丢失 |
| 复现频率 | 必现 |

对比正常场景：compact 完成后应立即显示压缩结果通知，不应长时间停留在 loading 状态。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 开始一个会话，进行多��对话产生足够上下文
  2. 在输入框输入 `/compact` 回车
  3. 观察聊天区域——显示 loading 骨架屏
  4. 等待 30 秒以上，最终恢复正常显示
- **环境**：TUI 模式

## 涉及文件

- `peri-tui/src/app/agent_compact.rs` — compact 生命周期处理，`handle_compact_completed` 在 full compact 时不调用 `set_loading(false)`
- `peri-tui/src/acp_server/compact.rs` — 手动 compact 执行入口，发送 `CompactStarted`/`CompactCompleted` 事件
- `peri-acp/src/session/executor.rs` — auto-compact 循环（手动 compact 不经过此路径）

## 关联 Issue

- `spec/issues/2026-05-25-compact-resubmit-missing-loading-spinner.md` — compact resubmit 时 spinner 缺失（相反方向的问题，都涉及 compact 与 loading 状态的协调）
