# /bg 后台 Fork Agent 斜杠命令实施计划

**状态**：Draft
**创建日期**：2026-06-06
**预估总 phase 数**：5
**来源**：brainstorming → [design doc](../specs/2026-06-06-bg-slash-command-design.md)

## 背景

用户需要在不中断主 Agent 工作流的前提下，主动派发独立任务到后台并行执行。现有 Agent 工具的后台模式仅能通过 LLM 决策触发，缺少用户主动发起的入口。新增 `/bg <prompt>` TUI 斜杠命令，fork 当前会话创建后台子 Agent。

## 目标

- [ ] `/bg <prompt>` 命令可用：fork 当前会话，后台执行任务，完成后按现有 bg agent 机制自动注入结果
- [ ] 提取共享 `spawner` 模块：`SubAgentTool` 和 `BgCommand` 共用后台 spawn 逻辑
- [ ] 定制 bg-fork directive：明确后台身份 + 面向主 Agent 的产出格式
- [ ] 现有 `invoke_background_fork` 改为调用共享 spawner

## 约束

- 不改变现有 bg agent 行为（结果注入、TUI 显示、并发限制）
- 后台 Agent cancel policy 必须为 Independent
- 上下文注入仅含对话历史 + bg-fork directive，不含额外摘要
- 必须通过 `cargo build -p peri-acp` 和 `cargo build -p peri-middlewares` 编译通过
- CJK 字符串截断必须用 `s.chars().take(N).collect()`

## 阶段总览

| Phase | 标题 | 涉及文件数 | 风险 |
|-------|------|-----------|------|
| 1 | 创建 spawner 共享模块 + bg-fork directive | 3 | 中 |
| 2 | 扩展 CommandContext + 前置 bg 通道创建 | 3 | 高 |
| 3 | 实现 BgCommand | 2 | 低 |
| 4 | 重构 execute_bg 使用 spawner | 1 | 中 |
| 5 | 测试 | 3 | 低 |

---

### Phase 1: 创建 spawner 共享模块 + bg-fork directive

**目标**：提取后台 fork Agent 的构建+spawn 逻辑为共享自由函数，供 BgCommand 和 SubAgentTool 共用。

**依赖**：无

**涉及文件**：
- `peri-middlewares/src/subagent/spawner.rs` —— **新增**：`BgForkConfig` 结构体 + `spawn_background_fork()` 函数
- `peri-middlewares/src/subagent/fork.rs` —— 修改：新增 `build_bg_fork_directive()` 函数
- `peri-middlewares/src/subagent/mod.rs` —— 修改：`pub mod spawner;`

**实施步骤**：

1. 在 `fork.rs` 新增 `build_bg_fork_directive(prompt: &str) -> String`，返回定制 directive 文本（含"后台异步 Agent"身份说明 + 结论/详细说明/关键文件/建议输出格式）。保留原有 `build_fork_directive` 不变。

2. 创建 `spawner.rs`，定义：
   ```rust
   pub struct BgForkConfig {
       pub prompt: String,
       pub parent_messages: Vec<BaseMessage>,
       pub cwd: PathBuf,
       pub llm: Arc<dyn BaseModel>,            // 子 Agent 使用的 LLM
       pub max_iterations: usize,               // 默认 200
       pub parent_tools: Arc<Vec<Arc<dyn BaseTool>>>,
       pub registered_hooks: Arc<Vec<RegisteredHook>>,
       pub thread_store: Option<Arc<dyn ThreadStore>>,
       pub parent_thread_id: Option<String>,
       pub register_runtime: Option<Arc<dyn Fn(&str, AgentCancellationToken) -> io::Result<()> + Send + Sync>>,
       pub bg_event_sender: UnboundedSender<AgentEvent>,
       pub bg_registry: Arc<BackgroundTaskRegistry>,
   }
   
   pub struct BgForkSpawned {
       pub task_id: String,
       pub child_thread_id: String,
   }
   
   pub async fn spawn_background_fork(config: BgForkConfig) -> Result<BgForkSpawned>
   ```

3. 实现 `spawn_background_fork()`：
   - 生成 `task_id`（`bg-{uuid}`）和 `child_thread_id`（UUID v7）
   - 并发检查：`bg_registry.active_count() >= 3` → 返回 `Err("已有 3 个后台任务在运行")`
   - 通过 `thread_store` 创建子线程（`hidden=true`, `cancel_policy=Independent`）
   - 构建 `BgTask` struct 注册到 `bg_registry.register()`
   - 构建 `ReActAgent`：`ReActAgent::new(llm).max_iterations(max_iterations)` + `build_subagent_middlewares(SubAgentMiddlewareConfig::for_fork(&cwd))`
   - 注册 parent_tools 到子 agent：`agent.register_tools(parent_tools.clone())`
   - 设置 event handler（带 source_agent_id）
   - 创建 `fork_state = AgentState::new(cwd)`，注入 `parent_messages` + `AgentInput::text(bg_fork_directive)`
   - `tokio::spawn` 执行 agent：
     - 执行完成后构造 `BackgroundTaskResult`
     - 调用 `fire_subagent_lifecycle_hooks_static(SubagentStop, ...)`
     - `bg_registry.complete(&task_id, result)`
     - `bg_event_sender.send(AgentEvent::BackgroundTaskCompleted(result))`
     - update_thread_status（done/cancelled/error）
     - deregister_runtime
   - 返回 `BgForkSpawned { task_id, child_thread_id }`

**验证**：
- `cargo build -p peri-middlewares` 编译通过
- 此时 spawner 函数未被调用，不影响现有行为

**风险**：函数参数过多（~12 个字段）。→ 允许：这是配置 struct 的合理设计，后续可用 builder 精简。

---

### Phase 2: 扩展 CommandContext + 前置 bg 通道创建

**目标**：让 Immediate 命令（BgCommand）能访问后台任务基础设施。当前 bg 通道在 `build_agent()` 内部创建，命令拦截在此之前。

**依赖**：无（与 Phase 1 并行，不同 crate）

**涉及文件**：
- `peri-acp/src/session/command/mod.rs` —— 修改：CommandContext 新增 2 个 Option 字段
- `peri-acp/src/session/executor.rs` —— 修改：命令拦截前创建 bg 通道，传入 CommandContext 和 build_agent
- `peri-acp/src/agent/builder.rs` —— 修改：build_agent 接收可选 prebuilt 通道参数

**实施步骤**：

1. **mod.rs**：在 `CommandContext` 末尾新增字段：
   ```rust
   pub bg_event_sender: Option<UnboundedSender<ExecutorEvent>>,
   pub bg_registry: Option<Arc<BackgroundTaskRegistry>>,
   ```
   更新 `command/mod_test.rs` 中所有 `CommandContext` 构造点（3 处），补 `None`。

2. **executor.rs**：在命令拦截之前（当前 ~L168）新增通道创建：
   ```rust
   // 前置创建 bg 通道（BgCommand 等 Immediate 命令依赖）
   let (bg_notification_tx, bg_notification_rx) = tokio::sync::mpsc::unbounded_channel();
   let background_registry = Arc::new(BackgroundTaskRegistry::new(bg_notification_tx));
   let (bg_event_tx, bg_event_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutorEvent>();
   ```
   在 `CommandContext` 构造时填充：
   ```rust
   bg_event_sender: Some(bg_event_tx.clone()),
   bg_registry: Some(background_registry.clone()),
   ```
   在 `build_agent()` 调用时传入 prebuilt 通道。

3. **builder.rs**：
   - `build_agent()` 新增参数 `prebuilt_bg_channels: Option<(UnboundedSender<ExecutorEvent>, UnboundedReceiver<ExecutorEvent>, Arc<BackgroundTaskRegistry>)>`。
   - `Some(...)` 时直接使用，跳过内部 bg 通道创建（L332-339），但 `bg_notification_rx` 仍需传给 `ReActAgent::with_notification_rx()`。
   - `None` 时走旧逻辑（兼容子 Agent 构建等非 session 路径）。
   - 类型：`UnboundedSender<ExecutorEvent>` 就是 `bg_event_tx`，`UnboundedReceiver<ExecutorEvent>` 就是 `bg_event_rx`。

**验证**：
- `cargo build -p peri-acp` 编译通过
- `cargo test -p peri-acp --lib` 全部通过（尤其 command 测试）
- 正常 TUI 对话不受影响（bg 通道提前创建不影响功能）

**风险**：build_agent 参数变更影响面大。→ 用 `Option` 保持向后兼容，测试路径传 `None`。

---

### Phase 3: 实现 BgCommand

**目标**：实现 `/bg <prompt>` Immediate 命令，调用 spawner + bg 通道完成后台 Agent 启动。

**依赖**：Phase 1（spawner）+ Phase 2（CommandContext bg 字段）

**涉及文件**：
- `peri-acp/src/session/command/bg.rs` —— **新增**
- `peri-acp/src/session/command/mod.rs` —— 修改：注册 + `pub mod bg`

**实施步骤**：

1. 创建 `bg.rs`，实现 `BgCommand`：
   ```rust
   pub struct BgCommand;
   
   #[async_trait]
   impl AgentCommand for BgCommand {
       fn name(&self) -> &str { "bg" }
       fn aliases(&self) -> &[&str] { &["background"] }
       fn description(&self) -> &str { "Fork 当前会话启动后台子 Agent 执行独立任务" }
       fn kind(&self) -> CommandKind { CommandKind::Immediate }
       
       async fn execute(&self, mut ctx: CommandContext) -> Result<CommandResult> {
           let prompt = ctx.args.trim().to_string();
           if prompt.is_empty() {
               // 返回用法提示
               ctx.event_sink.push_event(ExecutorEvent::TextChunk {
                   text: "用法: /bg <任务描述>\n".into(),
                   source_agent_id: None,
               })?;
               ctx.event_sink.push_done(&ctx.session_id).await;
               return Ok(CommandResult { messages: ctx.history, stop_reason: PromptStopReason::EndTurn });
           }
           
           // 从 peri_config 构造 LLM
           let provider = /* 从 ctx.peri_config 构建 */;
           let llm = provider.into_model();
           
           // 调用 spawner
           let spawned = spawn_background_fork(BgForkConfig {
               prompt: prompt.clone(),
               parent_messages: ctx.history.clone(),
               cwd: PathBuf::from(&ctx.cwd),
               llm,
               max_iterations: 200,
               parent_tools: /* 从 ctx 获取或构造 */,
               registered_hooks: Arc::new(Vec::new()),
               thread_store: ctx.thread_store.clone(),
               parent_thread_id: ctx.thread_id.clone(),
               register_runtime: None,
               bg_event_sender: ctx.bg_event_sender.clone().expect("bg channels 由 executor 前置创建"),
               bg_registry: ctx.bg_registry.clone().expect("bg channels 由 executor 前置创建"),
           }).await?;
           
           // 确认消息
           let truncated: String = prompt.chars().take(80).collect();
           ctx.event_sink.push_event(ExecutorEvent::TextChunk {
               text: format!("◆ 后台任务已启动: {truncated}\n"),
               source_agent_id: None,
           })?;
           ctx.event_sink.push_done(&ctx.session_id).await;
           
           Ok(CommandResult { messages: ctx.history, stop_reason: PromptStopReason::EndTurn })
       }
   }
   ```

2. 在 `mod.rs` 中注册：
   - 添加 `pub mod bg;`
   - 在 `default_command_registry()` 中添加 `reg.register(Box::new(bg::BgCommand));`

3. **关键决策点**：`parent_tools` 来源。BgCommand 不持有完整工具链。需要从 `ctx.peri_config` 构造工具集，或新增 `CommandContext` 字段。

**验证**：
- `cargo build -p peri-acp` 编译通过
- TUI 手动测试：输入 `/bg 帮我搜索 Rust 2026 roadmap`，确认后台 Agent 启动
- 确认 `◆ 后台任务已启动` 消息显示
- 确认后台 Agent 完成后结果自动注入下一轮

**风险**：`parent_tools` 构建复杂度。→ 可能需要 Phase 2 中额外传入工具集到 CommandContext，或 BgCommand 自己构建最小工具集（Read/Write/Bash/Grep/Glob/WebSearch/WebFetch）。优先方案：BgCommand 构造核心工具集（相当于 general-purpose agent）。

---

### Phase 4: 重构 execute_bg 使用 spawner

**目标**：消除重复代码，让 `SubAgentTool::invoke_background_fork()` 调用共享的 `spawn_background_fork()`。

**依赖**：Phase 1（spawner）

**涉及文件**：
- `peri-middlewares/src/subagent/tool/execute_bg.rs` —— 修改

**实施步骤**：

1. 重构 `invoke_background_fork()`：
   - 保留参数解包（从 `self` 提取 `parent_messages`、`cwd`、`llm_factory`、`parent_tools`、`registered_hooks`、`thread_store`、`register_runtime`、`bg_event_sender`）
   - 构造 `BgForkConfig` 并调用 `spawn_background_fork(config).await`
   - 格式化 tool result 字符串返回

2. 同样重构 `invoke_background()`（非 fork 路径），如果可共用 spawner。评估后决定：非 fork 路径差异较大（需加载 agent def、过滤工具、不同中间件配置），可后续优化。

**验证**：
- `cargo build -p peri-middlewares` 编译通过
- `cargo test -p peri-middlewares --lib` 全部通过
- 现有 SubAgent 后台行为不受影响

**风险**：invoke_background_fork 内部逻辑分散在多处。→ 逐行对比重构前后的行为，确保等价。

---

### Phase 5: 测试

**目标**：覆盖核心路径和边界情况。

**依赖**：Phase 1-4

**涉及文件**：
- `peri-middlewares/src/subagent/tool/fork_test.rs` —— **新增**
- `peri-acp/src/session/command/bg_test.rs` —— **新增**
- `peri-middlewares/src/subagent/spawner_test.rs` —— **新增**

**实施步骤**：

1. **fork_test.rs**：`build_bg_fork_directive` 文本测试
   - `test_bg_fork_directive_contains_prompt` — 验证 directive 包含用户原始 prompt
   - `test_bg_fork_directive_has_output_sections` — 验证包含"结论""详细说明""关键文件"章节
   - `test_bg_fork_directive_distinct_from_fork` — 验证与 `build_fork_directive` 不同

2. **bg_test.rs**：BgCommand 行为测试
   - `test_bg_command_empty_prompt` — 无参数返回用法提示
   - `test_bg_command_empty_prompt_pushes_done` — 验证 push_done 被调用
   - (不测试有参数 spawn，因为需要真实 LLM 环境)

3. **spawner_test.rs**：spawner 并发控制测试
   - `test_spawner_concurrent_limit` — `active_count >= 3` 时返回错误
   - 使用 mock BackgroundTaskRegistry

**验证**：
- `cargo test -p peri-middlewares --lib` 全部通过
- `cargo test -p peri-acp --lib` 全部通过

---

## 不在范围内

- `/bg --model haiku` 参数化模型选择
- `/bg --turns 50` 限制迭代次数
- 后台 Agent 取消命令（`/bg-cancel <task_id>`）
- TUI 通知改进（bg bar 高亮等）
- `invoke_background`（非 fork）的重构

## 开放问题

- [ ] **`parent_tools` 来源**（Phase 3）：BgCommand 需要构造子 Agent 的工具集。是让 CommandContext 暴露工具集，还是 BgCommand 自己构造核心工具集？倾向于后者（构造通用工具集），但需确认 `peri_config` 中工具注册逻辑的调用方式。
- [ ] **`llm_factory` 适配**（Phase 3）：BgCommand 通过 `ctx.peri_config` 构造 LLM。需确认 `peri_config` 到 `BaseModel` 的构造路径是否在 `peri-acp` crate 中可访问，还是需要通过 `builder.rs` 的 `build_agent` 片段。
