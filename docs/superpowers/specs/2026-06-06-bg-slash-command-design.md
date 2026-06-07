# /bg 后台 Fork Agent 斜杠命令设计

**状态**：Approved
**创建日期**：2026-06-06

## 概述

新增 TUI 斜杠命令 `/bg <prompt>`，fork 当前会话创建后台子 Agent 执行独立任务。后台 Agent 使用定制上下文封装，完成后按现有 bg agent 机制自动注入结果。

### 动机

用户希望在不中断主 Agent 工作流的前提下，派发独立任务到后台并行执行（如调研、分析、搜索）。现有 Agent 工具的后台模式需要通过 LLM 决策触发，缺少用户主动发起的入口。

---

## 架构

### 数据流

```
用户输入 "/bg 调研 Rust async trait"
  │
  ▼
execute_prompt() → 斜杠命令拦截
  │ "/bg" → BgCommand (CommandKind::Immediate)
  ▼
BgCommand.execute(ctx)
  ├─ 提取 prompt（"/bg" 后的全部文本）
  ├─ 获取父消息快照 (ctx.history)
  ├─ 构建 bg-fork directive（定制提示词）
  ├─ 创建子 AgentState + 注入历史
  ├─ spawn_background_fork() → tokio::spawn
  │    ├─ 子 Agent 独立执行（ReAct 循环）
  │    └─ 完成 → bg_event_sender.send(BackgroundTaskCompleted)
  ├─ event_sink.push_event(TextChunk: "◆ 后台任务已启动: ...")
  ├─ event_sink.push_done()
  └─ 返回 CommandResult::empty_end_turn()

后台完成路径（与现有 bg agent 一致）：
  bg_event_rx → TUI poll_background_events()
    → handle_background_task_completed()
      → pre_done_bg_results 缓冲
      → pending_bg_continuation → submit_bg_continuation()
      → 下一轮主 Agent 看到 AgentResult
```

### 组件关系

```
┌─────────────────────────────────────────────────┐
│ peri-acp/src/session/command/bg.rs              │
│ BgCommand: AgentCommand (Immediate)             │
│  → 解析 prompt, 构建 BgForkConfig               │
│  → 调用 spawn_background_fork()                 │
│  → push 确认消息 + push_done()                   │
└────────────────────┬────────────────────────────┘
                     │ 调用
┌────────────────────▼────────────────────────────┐
│ peri-middlewares/src/subagent/spawner.rs         │
│ pub async fn spawn_background_fork(             │
│     config: BgForkConfig,                       │
│ ) -> Result<BgForkSpawned>                      │
│  → 并发检查 (BackgroundTaskRegistry)             │
│  → 构建 bg-fork directive                       │
│  → 创建子 AgentState + 注入历史                  │
│  → 构建子 Agent (中间件链 + 工具 + 模型)          │
│  → thread_store 注册子线程                       │
│  → tokio::spawn → bg_registry.register()        │
│  → 返回 task_id                                 │
└────────────────────┬────────────────────────────┘
                     │ 替代
┌────────────────────▼────────────────────────────┐
│ peri-middlewares/src/subagent/tool/execute_bg.rs │
│ invoke_background_fork() 改为调用 spawner  API   │
└─────────────────────────────────────────────────┘
```

---

## 上下文封装

### bg-fork directive

定制的 fork directive，明确后台身份和面向主 Agent 的产出格式：

```markdown
<bg_fork_directive>
你是一个后台异步 Agent，正在为父会话执行一项独立任务。

## 你的身份
- 你在后台独立运行，无法与用户交互
- 你的结果会以工具结果的形式注入主 Agent 的下一轮对话
- 主 Agent 不会等待你——它可能已经继续工作了

## 对话上下文
以下是父会话的历史记录，帮助你理解当前讨论的背景。
请聚焦于你的任务，不要试图延续对话或回应用户。

## 任务
{用户的 prompt 原文}

## 规则
1. 只执行分配给你的任务，不做额外的事
2. 不要提问、不要请求澄清——基于已有信息做出最佳判断
3. 不要生成子 Agent
4. 如需文件编辑，谨慎操作

## 输出格式
你的响应会直接注入主会话。请用以下结构，确保主 Agent 可以无缝消费：

### 结论
<一句话总结核心发现或完成的工作>

### 详细说明
<具体内容——分析结果、代码变更说明、调研发现等>

### 关键文件
- `path/to/file` — 简要说明

### 建议（可选）
<如果有值得主 Agent 关注的后续步骤>

</bg_fork_directive>
```

### 与默认 fork directive 的差异

| 维度 | 默认 fork | bg-fork |
|------|-----------|---------|
| 身份说明 | "forked agent continuing" | "后台异步 Agent，无法交互" |
| 输出格式 | Scope/Result/Key files（英文、简版） | 结论/详细说明/关键文件/建议（中文、详细） |
| 上下文感知 | 无 | 提示"主 Agent 可能已继续工作" |
| 对话语义 | 强调"延续对话" | 强调"独立任务，结果注入" |

### 上下文注入

从 `ctx.history` 读取完整消息列表，全部注入子 AgentState。不额外注入上下文摘要——完整历史已足够让后台 Agent 理解讨论背景。子 Agent 内部消息隔离（独立 AgentState），完成后仅结果文本返回。

---

## BgForkConfig API

```rust
/// 后台 Fork Agent 配置
pub struct BgForkConfig {
    /// 用户任务描述（/bg 后的文本）
    pub prompt: String,
    /// 父会话消息快照
    pub parent_messages: Vec<BaseMessage>,
    /// 工作目录
    pub cwd: PathBuf,
    /// Provider/Model 配置
    pub peri_config: Arc<RwLock<PeriConfig>>,
    /// 线程注册
    pub thread_store: ThreadStore,
    /// 父线程 ID
    pub parent_thread_id: String,
    /// 父取消令牌（用于创建子 cancel token）
    pub parent_cancel_token: Option<AgentCancellationToken>,
    /// 后台事件发送通道
    pub bg_event_sender: UnboundedSender<AgentEvent>,
    /// 后台任务注册表（并发控制）
    pub bg_registry: Arc<BackgroundTaskRegistry>,
}

pub struct BgForkSpawned {
    pub task_id: String,
    pub child_thread_id: String,
}

/// 启动后台 Fork Agent。返回 task_id 供注册表追踪。
pub async fn spawn_background_fork(
    config: BgForkConfig,
) -> Result<BgForkSpawned>;
```

### 内置规则

1. **并发限制**：`BackgroundTaskRegistry.active_count() < 3`，超限返回错误
2. **Cancel 策略**：子 Agent 使用 `Independent` cancel policy（父 cancel 不影响后台任务）
3. **线程注册**：`thread_store` 创建子线程（`hidden=true`, `cancel_policy=Independent`）
4. **事件路由**：子 Agent 事件通过 `bg_event_sender` 发送，不经过主 event pump
5. **模型选择**：默认继承父 Agent 的模型配置（`peri_config` 快照）

---

## BgCommand 实现

```rust
// peri-acp/src/session/command/bg.rs

pub struct BgCommand;

#[async_trait]
impl AgentCommand for BgCommand {
    fn name(&self) -> &str { "bg" }
    fn aliases(&self) -> &[&str] { &["background"] }
    fn description(&self) -> &str {
        "Fork 当前会话启动后台子 Agent 执行独立任务"
    }
    fn kind(&self) -> CommandKind { CommandKind::Immediate }

    async fn execute(&self, mut ctx: CommandContext) -> Result<CommandResult> {
        let prompt = ctx.args.trim().to_string();

        // 空参数：返回用法提示
        if prompt.is_empty() {
            ctx.event_sink.push_event(ExecutorEvent::TextChunk {
                text: "用法: /bg <任务描述>\n".into(),
                source_agent_id: None,
            })?;
            ctx.event_sink.push_done()?;
            return Ok(CommandResult::empty_end_turn());
        }

        // 启动后台 Fork Agent
        let spawned = spawn_background_fork(BgForkConfig {
            prompt: prompt.clone(),
            parent_messages: ctx.history,
            cwd: ctx.cwd,
            peri_config: ctx.peri_config,
            thread_store: ctx.thread_store,
            parent_thread_id: ctx.thread_id,
            parent_cancel_token: ctx.cancel_token,
            bg_event_sender: ctx.bg_event_sender
                .expect("bg_event_sender 总是 Some（executor 前置创建）"),
            bg_registry: ctx.bg_registry
                .expect("bg_registry 总是 Some"),
        }).await.map_err(|e| {
            // 并发超限等错误：转为用户可见消息
            ctx.event_sink.push_event(ExecutorEvent::TextChunk {
                text: format!("✗ 后台任务启动失败: {e}\n"),
                source_agent_id: None,
            }).ok();
            anyhow!("{e}")
        })?;

        // 确认消息
        let truncated = truncate_str(&prompt, 80);
        ctx.event_sink.push_event(ExecutorEvent::TextChunk {
            text: format!("◆ 后台任务已启动: {truncated}\n"),
            source_agent_id: None,
        })?;
        ctx.event_sink.push_done()?;

        Ok(CommandResult::empty_end_turn())
    }
}
```

---

## CommandContext 扩展

新增两个 `Option` 字段：

```rust
pub struct CommandContext {
    // ... 现有字段 ...
    /// 后台任务事件发送通道（BgCommand 依赖，由 executor 前置创建）
    pub bg_event_sender: Option<UnboundedSender<ExecutorEvent>>,
    /// 后台任务注册表（并发控制）
    pub bg_registry: Option<Arc<BackgroundTaskRegistry>>,
}
```

### executor.rs 变更：前置 bg 通道创建

当前 bg 通道在 `build_agent()` 内部创建（builder.rs:329-339，位于命令拦截之后）。BgCommand 作为 Immediate 命令执行在 build_agent 之前，需要这些通道已存在。

**变更**：将通道创建从 `builder.rs` 前移到 `executor.rs` 命令拦截之前（~L168），通过 `AcpAgentBuilder` 参数传入：

```rust
// executor.rs: 命令拦截之前新增
let (bg_notification_tx, _bg_notification_rx) = tokio::sync::mpsc::unbounded_channel();
let background_registry = Arc::new(BackgroundTaskRegistry::new(bg_notification_tx));
let (bg_event_tx, bg_event_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutorEvent>();

// 传入 CommandContext
let ctx = CommandContext {
    // ... existing fields ...
    bg_event_sender: Some(bg_event_tx.clone()),
    bg_registry: Some(background_registry.clone()),
};

// 传入 build_agent（替代 builder.rs 内部的创建）
let agent_output = build_agent(AcpAgentConfig {
    // ...
    prebuilt_bg_channels: Some((bg_event_tx, bg_event_rx, background_registry)),
}).await?;
```

`AcpAgentBuilder::build_agent()` 接收 `prebuilt_bg_channels` 参数：存在时直接使用，`None` 时走旧逻辑自行创建（兼容测试、子 Agent 构建等路径）。

---

## 边界情况

| 场景 | 处理 |
|------|------|
| `/bg` 无参数 | 推送提示 "用法: /bg <任务描述>" |
| 并发超限 (≥3) | spawner 返回错误，推送 "已有 3 个后台任务在运行" |
| 父会话无历史 | 正常 fork（空历史），子 Agent 只有 bg-fork directive |
| 命令执行中 panic | Immediate 命令在 try 块内，错误被捕获返回 |
| 后台 Agent panic | JoinHandle 返回 Err，通过 bg_event_sender 发送失败事件 |
| 父 Agent cancel | 后台 Agent 使用 Independent policy，不受影响 |

---

## 现有代码重构

### spawner 模块提取

`execute_bg.rs::invoke_background_fork()` 改为调用 `spawner::spawn_background_fork()`：

```rust
// Before: invoke_background_fork 包含全部构建 + spawn 逻辑
// After: 仅保留参数解包 + 结果格式化
pub(crate) async fn invoke_background_fork(&self, args: &AgentArgs) -> Result<String> {
    // 参数解包
    let spawned = spawn_background_fork(BgForkConfig {
        prompt: args.prompt.clone(),
        parent_messages: self.parent_messages.read().clone(),
        // ...
    }).await?;

    // 格式化 tool result
    Ok(format!("Background task started: {}\nchild_thread_id: {}",
        spawned.task_id, spawned.child_thread_id))
}
```

SubAgentTool 的 `invoke_background`（非 fork）也同样改造。

---

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `peri-acp/src/session/command/bg.rs` | **新增** | BgCommand 实现 |
| `peri-acp/src/session/command/mod.rs` | 修改 | 注册 BgCommand + `pub mod bg` + 新增 `CommandContext` 字段 |
| `peri-acp/src/session/executor.rs` | 修改 | 前置 bg 通道创建 + CommandContext 填充 |
| `peri-acp/src/agent/builder.rs` | 修改 | build_agent 接收 `prebuilt_bg_channels` 参数 |
| `peri-middlewares/src/subagent/spawner.rs` | **新增** | 共享后台 spawn API |
| `peri-middlewares/src/subagent/mod.rs` | 修改 | `pub mod spawner` |
| `peri-middlewares/src/subagent/tool/execute_bg.rs` | 修改 | invoke_background_fork → 调 spawner |
| `peri-middlewares/src/subagent/tool/fork.rs` | 修改 | 新增 `build_bg_fork_directive()` |
| `peri-middlewares/src/subagent/tool/fork_test.rs` | **新增** | directive 文本测试 |
| `peri-acp/src/session/command/bg_test.rs` | **新增** | BgCommand 空参数测试 |
| `peri-middlewares/src/subagent/spawner_test.rs` | **新增** | 并发限制测试 |

---

## 测试

| 测试 | 位置 | 内容 |
|------|------|------|
| `test_bg_fork_directive_contains_prompt` | `fork_test.rs` | directive 包含用户 prompt 原文 |
| `test_bg_fork_directive_has_output_format` | `fork_test.rs` | directive 包含"结论/详细说明/关键文件"章节 |
| `test_bg_command_empty_prompt` | `bg_test.rs` | 无参数返回用法提示 |
| `test_bg_command_normal_prompt` | `bg_test.rs` | 有参数成功 spawn |
| `test_spawner_concurrent_limit` | `spawner_test.rs` | 并发超限返回错误 |
| `test_spawner_cancel_policy_independent` | `spawner_test.rs` | 子 Agent 使用 Independent policy |

**不测试**：后台 Agent 执行结果正确性（依赖 LLM）、bg event 链路（已有验证）。

---

## 后续扩展（不在本次 scope）

- `/bg --model haiku` 指定后台 Agent 模型
- `/bg --turns 50` 限制后台 Agent 迭代次数
- 后台 Agent 完成后 TUI 通知改进（如高亮 bg bar）
- 后台 Agent 取消命令（`/bg-cancel <task_id>`）
