# SystemNote 在 RebuildAll 后堆积到消息列表末尾

**状态**：Open
**优先级**：中
**创建日期**：2026-05-12

## 问题描述

TUI 层通过 `AddMessage(MessageViewModel::system(...))` 添加的 `SystemNote`（OAuth 通知、错误提示、配置操作反馈等）在 `RebuildAll` 触发后被推到 `view_messages` 的末尾，而不是保持在其产生时的位置。无论 agent 运行中还是 agent 完成后操作面板，都存在此问题。

## 症状详情

### 当前行为

1. agent 运行过程中产生 SystemNote（如 OAuth 通知），被 `push` 到 `view_messages` 末尾
2. `RebuildAll` 触发时，`apply_pipeline_action` 在 `agent_render.rs:70-76` 将 `prefix_len..` 范围内的 `SystemNote` 过滤保存
3. 保存的 `SystemNote` 在 `agent_render.rs:101` 被追加到 `view_messages` 末尾
4. 结果：SystemNote 永远出现在所有消息的最后面

### 期望行为

SystemNote 应保持在它被创建时的位置附近，不会因为后续的 RebuildAll 而漂移到末尾。

## 相关代码

- `rust-agent-tui/src/app/agent_render.rs:43-48` —— `AddMessage` 直接 push 到 view_messages 末尾
- `rust-agent-tui/src/app/agent_render.rs:70-101` —— `RebuildAll` 的 `saved_notes` 机制：保存被 drain 的 SystemNote 并追加到末尾
- `rust-agent-tui/src/app/message_pipeline.rs:61` —— `PipelineAction::AddMessage` 定义

### AddMessage 的产生点（部分）

| 文件 | 场景 |
|------|------|
| `agent_events_oauth.rs:33,54,83` | OAuth 完成/失败/操作 |
| `agent_events_plugin.rs:111` | Plugin 操作结果 |
| `agent_ops.rs:170` | Prompt cache 命中率警告 |
| `agent_ops.rs:536,539,544` | 中断通知 |
| `agent_compact.rs:176,209,267` | 压缩相关通知 |
| `agent_submit.rs:105` | 未配置 API Key 提示 |
| `agent_panel.rs:132,143` | Agent 重置/切换通知 |
| `model_panel.rs:264,273` | 模型切换通知 |
| `login_panel.rs:433,446,568,583,636,649` | Provider 管理 |
| `config_panel.rs:336,344` | 配置保存 |
| `panel_ops.rs:51,61,113` | 面板操作反馈 |
| `plugin_panel.rs:923,974,1005,1188` | 插件管理 |
| `cron_ops.rs:52` | Cron 任务删除 |
| `cron_state.rs:169` | Cron 任务操作 |
| `thread_ops.rs:311,329,365` | 压缩/历史操作 |

## 修复方案

### 方案：VM 索引锚点

为 `AddMessage` 路径的 `SystemNote` 记录创建时的 `view_messages` 索引位置作为锚点。`RebuildAll` 时根据锚点将 SystemNote 插入到 `tail_vms` 的对应位置。

**关键设计**：

1. **新增 `PipelineAction::InsertMessage { vm, anchor_vm_idx }`**：替代 `AddMessage`，携带锚点索引
2. **锚点语义**：`anchor_vm_idx` = SystemNote 被创建时 `view_messages.len()`，即它应该插入的位置
3. **RebuildAll 时的处理**：将 `saved_notes` 的锚点位置转换为 `tail_vms` 内的插入索引（`anchor - prefix_len`），插入到对应位置
4. **冲突处理**：如果锚点位置在 prefix 范围内（已被 drain），丢弃该 SystemNote
5. **不破坏持久化数组**：SystemNote 是纯 UI 层概念，不进入 `BaseMessage[]`，不影响持久化
