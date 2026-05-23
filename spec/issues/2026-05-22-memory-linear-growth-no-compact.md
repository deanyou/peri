# 长对话内存持续增长，无自动释放机制

**状态**：Open
**优先级**：中
**类型**：性能
**创建日期**：2026-05-22

## 问题描述

Agent 对话过程中，内存（RSS）随对话轮数线性增长，每轮约增长几十 MB，且不会自动下降。持续跑 50-100 轮对话后可达数 GB，最终导致 OOM。`/clear` 或 `new_thread` 能手动释放全部内存，但对话过程中缺乏自动的上下文压缩（compact）机制来限制内存使用。

## 症状详情

| 维度 | 观察 |
|------|------|
| 增长模式 | 对话轮数相关，非时间相关 |
| 增长速度 | ~几十 MB/轮 |
| 是否自动下降 | 否，只增不减 |
| 触发场景 | 各类操作均有（SubAgent/大文件读取/纯文本） |
| 手动缓解 | `/clear` (new_thread) 可完全释放 |

## 数据结构分析

每轮对话后，消息数据同时存在至少 4 个位置：

| 位置 | 类型 | 更新方式 | 文件 |
|------|------|----------|------|
| `SessionState.history` | `Vec<BaseMessage>` | 整体替换 `=` | `peri-tui/src/acp_server/mod.rs:44` |
| `AgentComm.agent_state_messages` | `Vec<BaseMessage>` | 增量 `extend()` | `peri-tui/src/app/agent_comm.rs:37` |
| `MessagePipeline.completed` | `Vec<BaseMessage>` | `restore_completed()` | pipeline 内部 |
| `message_state.view_messages` | `Vec<MessageViewModel>` | 增量追加 | `peri-tui/src/app/message_state.rs:13` |

假设每轮对话产生 30MB 消息（含 tool result），50 轮后内存占用约 30MB × 4 × 50 = 6GB。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI，正常对话
  2. 每发一轮消息，观察 RSS 增长
  3. 持续对话数轮后，RSS 持续上升
  4. `/clear` 后 RSS 下降——验证非真正泄漏
- **环境**：macOS，Rust 2021，任何模型下均出现

### 现象 2（2026-05-23）：debug 模式下 `/clear` 后 RSS 不下降

| 维度 | 观察 |
|------|------|
| 编译模式 | debug（`./dev.sh` 启动） |
| `/clear` 前 RSS | 几百 MB |
| `/clear` 后 RSS | 无明显变化，仍在几百 MB |
| 与 release 对比 | 未对比，待确认 release 下 `/clear` 是否能正常释放 |

**推测**：debug 模式下无优化，Rust 全局分配器（jemalloc/system allocator）倾向于保留已释放的内存页不归还 OS，导致 RSS 数值不降。需对比 release 模式确认是否为 debug 专属现象。

## 改进方向

- 实现或启用 compact 机制：在达到上下文预算阈值时自动压缩历史消息
- 或消除消息的多副本存储（如共享 `Arc<Vec<BaseMessage>>` 而非独立 clone）
- 当前 `/clear` (new_thread) 可作为手动缓解手段，但缺乏自动化

## 涉及文件

- `peri-tui/src/acp_server/mod.rs` —— ACP 服务器端 SessionState.history
- `peri-tui/src/app/agent_comm.rs` —— TUI 端 agent_state_messages
- `peri-tui/src/app/agent_submit.rs` —— submit_message 流程
- `peri-tui/src/app/thread_ops.rs` —— new_thread（/clear）释放逻辑
- `peri-tui/src/acp_server/prompt.rs` —— 每轮执行后 state.history 更新
