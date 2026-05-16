# ACP ↔ TUI Agent 构建复用计划

> 目标：ACP 路径与 TUI 路径共用同一套 Agent 构建逻辑，消除重复
> 涉及文件：`peri-tui/src/app/agent.rs`、`peri-tui/src/acp/agent_assembler.rs`

---

## 现状

```
TUI 路径 (agent.rs)              ACP 路径 (agent_assembler.rs)
─────────────────────            ─────────────────────────────
run_universal_agent()            assemble_agent()
  ├─ 构建 event_handler            ├─ 构建 event_handler (ACP 版)
  ├─ 构建 model (LLM)              ├─ 构建 model (LLM) ← 重复
  ├─ 构建 broker (Tui)             ├─ 构建 broker (ACP 版，外部传入)
  ├─ 构建 HITL                     ├─ 构建 HITL ← 重复
  ├─ 构建 AskUserTool              ├─ 构建 AskUserTool ← 重复
  ├─ 构建 SubAgent (完整版)         ├─ 构建 SubAgent (精简版) ← 重复
  ├─ 构建 ReActAgent (完整版)       ├─ 构建 ReActAgent (精简版) ← 缺失 5 配置 + 5 中间件
  ├─ + LSP/Hooks/Langfuse          ├─ 返回 executor
  └─ execute() + Done/Error 事件    └─ 外部 execute() + PromptResponse
```

## 设计：提取 `build_bare_agent()` 共享函数

### 签名

```rust
pub fn build_bare_agent(config: BareAgentConfig) -> BareAgentOutput
```

### 输入：`BareAgentConfig`（ACP 和 TUI 的共同输入）

```rust
pub struct BareAgentConfig {
    pub provider: LlmProvider,
    pub cwd: String,
    pub system_prompt: String,
    pub event_handler: Arc<dyn AgentEventHandler>,
    pub cancel: AgentCancellationToken,
    pub permission_mode: Arc<SharedPermissionMode>,
    pub peri_config: Arc<PeriConfig>,
    pub cron_scheduler: Option<Arc<Mutex<CronScheduler>>>,
    pub agent_overrides: Option<AgentOverrides>,
    pub preload_skills: Vec<String>,
    pub session_id: Option<String>,
    pub broker: Arc<dyn UserInteractionBroker>,
    // 可选（ACP 可传默认值）：
    pub plugin_skill_dirs: Vec<PathBuf>,         // ACP: vec![]
    pub plugin_agent_dirs: Vec<PathBuf>,          // ACP: vec![]
    pub hook_groups: Vec<Vec<RegisteredHook>>,    // ACP: vec![]
    pub hook_session_start: bool,                 // ACP: false
    pub mcp_pool: Option<Arc<McpClientPool>>,     // ACP: None, TUI: from MCP init
    pub tool_search_index: Arc<ToolSearchIndex>,  // ACP: new session-level
    pub shared_tools: Arc<RwLock<HashMap<String, Arc<dyn BaseTool>>>>, // ACP: new
}
```

### 输出：`BareAgentOutput`

```rust
pub struct BareAgentOutput {
    pub executor: ReActAgent<RetryableLLM<BaseModelReactLLM>, AgentState>,
    pub todo_rx: mpsc::Receiver<Vec<TodoItem>>,
    pub model: RetryableLLM<BaseModelReactLLM>,
    pub context_window: u32,
}
```

### 内部构建逻辑（与当前 TUI agent.rs:296-390 完全对齐）

```
ReActAgent::new(model)
    .max_iterations(500)
    .with_context_budget(budget)          ← ACP 新增
    .with_compact_config(compact)         ← ACP 新增
    .with_notification_rx(bg_rx)          ← shared (bg task support)
    .with_system_prompt(prompt)
    .with_tool_filter(is_deferred_tool)   ← ACP 新增
    .with_shared_tools(shared_tools)      ← ACP 新增
    .add_middleware(AgentsMdMiddleware)
    .add_middleware(AgentDefineMiddleware)
    .add_middleware(SkillsMiddleware + extra_dirs)
    .add_middleware(SkillPreloadMiddleware)
    .add_middleware(FilesystemMiddleware)
    .add_middleware(GitAttributionMiddleware)
    .add_middleware(TerminalMiddleware)
    .add_middleware(WebMiddleware)         ← ACP 新增
    .add_middleware(TodoMiddleware)
    .add_middleware(CronMiddleware)
    + HookMiddleware * N (if hook_groups not empty)
    + HITL
    + SubAgent (完整版，含 .with_parent_messages/.with_background_registry/.with_registered_hooks)
    + McpMiddleware (if pool exists)       ← ACP 新增
    + ToolSearchMiddleware                 ← ACP 新增
    .register_tool(AskUserTool)
```

---

## 修改步骤

### 步骤 1：在 `agent.rs` 中创建 `BareAgentConfig`、`BareAgentOutput`、`build_bare_agent()`
- 从 `run_universal_agent` 的 296-390 行提取 ReActAgent 构建逻辑
- 移除 TUI 特有逻辑（Langfuse handler、LSP、Done/Error event sending）
- `with_event_handler` 保留（由调用方传入）

### 步骤 2：重构 `run_universal_agent` 调用 `build_bare_agent()`
- 删去重复的 Agent 构建代码
- 保留：Langfuse tracing wrapper、LSP middleware 附加、execute + Done/Error 事件
- ACP 会从 `BareAgentOutput` 拿到 executor 自行 execute

### 步骤 3：删除 `agent_assembler.rs` 中的重复代码
- `assemble_agent()` 调用 `build_bare_agent()`
- 删去 LLM 构建、HITL 构建、SubAgent 构建、ReActAgent 构建中的重复逻辑
- `AgentAssembleConfig` 可以简化为包装 `BareAgentConfig`

### 步骤 4：构建验证
```bash
cargo build -p peri-tui
cargo test -p peri-tui
```

---

## 两项路径差异保留

| 差异点 | TUI | ACP | 处理方式 |
|--------|-----|-----|---------|
| event_handler | Langfuse + AgentEvent mapping | SessionNotification mapping | 各自构建，传入 `BareAgentConfig` |
| broker | `TuiInteractionBroker` | `AcpInteractionBroker` | 各自构建，传入 `BareAgentConfig` |
| post-execute | `tx.send(Done/Error/Interrupted)` | `responder.respond(PromptResponse)` | 各自在 `build_bare_agent()` 之外处理 |
| Langfuse | ✅ | ❌ | TUI 侧 wrapper |
| LSP | ✅ | ❌ | TUI 在 `build_bare_agent()` 之后附加 |
| plugin_* | ✅ 从配置加载 | ❌ 传空 | 通过参数区分 |

---

## 风险

- **HookMiddleware 的 LLM 工厂复用**：当前 TUI 路径的 Hook LLM factory 在 `run_universal_agent` 内构建。提取后需要确保 HookMiddleware 所需参数全部通过 `BareAgentConfig` 传入。
- **SubAgent 的 `with_registered_hooks`**：ACP 当前不传 `plugin_hooks`，传入空 vec 即可保持兼容。
- **parent_messages / background_registry**：ACP 不需要（无后台任务），传空 Arc 即可。
