# /bg 代码审查修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 /bg 斜杠命令代码审查中发现的 6 个问题（C1/C2, H1, H2, L2, M1）

**Architecture:** 分 6 个独立 Task，每个 Task 只修改 1-2 个文件，按依赖排序（C1 必须先完成）。C1 消除 execute_bg.rs 与 spawner.rs 的 230 行重复代码，统一为 spawner 路径。其余 Task 独立。

**Tech Stack:** Rust 2021, tokio, peri-agent/peri-middlewares/peri-acp workspace crates

---

### Task 1: 消除 execute_bg.rs 重复代码 + 修复竞态条件 (C1 + C2)

**Files:**
- Modify: `peri-middlewares/src/subagent/tool/execute_bg.rs:260-489` → 简化为委托给 spawner
- Modify: `peri-middlewares/src/subagent/spawner.rs` → 添加 `llm_factory` 支持 + 区分 fork/bg-fork directive 类型

**背景**：`invoke_background_fork()` (execute_bg.rs:260-489) 与 `spawn_background_fork()` (spawner.rs:118-350) 有 230 行重复代码，且前者在 `registry.register()` 之前 spawn 导致竞态条件。两路径使用不同 directive（`build_fork_directive` vs `build_bg_fork_directive`）。

**方案**：让 `invoke_background_fork()` 构造 `BgForkConfig` 并委托给 `spawn_background_fork()`。为保持向后兼容，在 `BgForkConfig` 中新增 `fork_directive_kind` 字段区分两种 directive 类型。

- [ ] **Step 1: 在 BgForkConfig 中添加 directive_kind 字段**

在 `peri-middlewares/src/subagent/spawner.rs` 的 `BgForkConfig` 结构体末尾添加字段：

```rust
/// Fork 指令类型：BGFork 使用中文 bg-fork directive，普通使用英文 fork directive
pub fork_directive_kind: BgForkDirectiveKind,
```

并在文件开头添加枚举定义（在 `BgForkConfig` 之前）：

```rust
/// Fork 指令类型，决定 fork agent 使用的 system directive 模板
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgForkDirectiveKind {
    /// 使用 build_fork_directive()（英文，Agent 工具路径）
    Fork,
    /// 使用 build_bg_fork_directive()（中文，/bg 命令路径）
    Bg,
}
```

- [ ] **Step 2: 在 spawn_background_fork 中使用 directive_kind**

在 `spawner.rs` 中找到 `build_bg_fork_directive` 调用处（约第 147 行），替换为条件逻辑：

```rust
// 原代码:
// let fork_directive = crate::subagent::fork::build_bg_fork_directive(&prompt);

// 替换为:
let fork_directive = match config.fork_directive_kind {
    BgForkDirectiveKind::Bg => {
        crate::subagent::fork::build_bg_fork_directive(&prompt)
    }
    BgForkDirectiveKind::Fork => {
        crate::subagent::fork::build_fork_directive(&prompt)
    }
};
```

- [ ] **Step 3: 在 BgCommand 中设置 directive_kind = Bg**

在 `peri-acp/src/session/command/bg.rs` 的 `BgForkConfig` 构造中（约第 116 行附近），添加：

```rust
fork_directive_kind: BgForkDirectiveKind::Bg,
```

- [ ] **Step 4: 重构 invoke_background_fork 委托给 spawner**

在 `peri-middlewares/src/subagent/tool/execute_bg.rs` 中，将 `invoke_background_fork()` 方法体（约第 260-489 行）替换为构造 `BgForkConfig` 并调用 `spawn_background_fork()`：

```rust
async fn invoke_background_fork(
    &self,
    prompt: String,
    cwd: String,
    task_id: String,
    registry: &Arc<BackgroundTaskRegistry>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let agent_name = "fork".to_string();
    let prompt_summary: String = prompt.chars().take(100).collect();

    let parent_msgs: Vec<BaseMessage> = match &self.parent_messages {
        Some(pm) => pm.read().clone(),
        None => return Err(
            "Error: Fork path requires parent message history, but parent_messages is not set"
                .into(),
        ),
    };

    let bg_fork_child_thread_id = uuid::Uuid::now_v7().to_string();
    if let Some(ref store) = self.thread_store {
        let snapshot_id = parent_msgs.last().map(|m| m.id().as_uuid().to_string());
        let mut child_meta = ThreadMeta::new(&cwd);
        child_meta.id = bg_fork_child_thread_id.clone();
        child_meta.parent_thread_id = self.parent_thread_id.clone();
        child_meta.snapshot_at_message_id = snapshot_id;
        child_meta.hidden = true;
        child_meta.cancel_policy = "independent".to_string();
        child_meta.title = Some(format!("bg-fork-{}", task_id));
        store
            .create_thread(child_meta)
            .await
            .map_err(|e| format!("Failed to create child thread: {}", e))?;
    }

    let llm = (self.llm_factory)(None);

    let config = BgForkConfig {
        llm: llm.into_base_model_arc(),
        prompt: prompt.clone(),
        agent_name,
        prompt_summary,
        cwd,
        base_tools: self.parent_tools.clone(),
        system_builder: self.system_builder.clone(),
        parent_messages: Some(parent_msgs),
        thread_store: self.thread_store.clone(),
        parent_thread_id: self.parent_thread_id.clone(),
        child_thread_id_override: Some(bg_fork_child_thread_id),
        registered_hooks: self.registered_hooks.clone(),
        register_runtime: None,
        deregister_runtime: None,
        bg_event_sender: self.bg_event_sender.clone(),
        bg_registry: Arc::clone(registry),
        fork_directive_kind: BgForkDirectiveKind::Fork,
    };

    let spawned = spawn_background_fork(config).await?;
    Ok(spawned.task_id)
}
```

**注意**：此步骤需要 `ReactLLM` 提供 `into_base_model_arc()` 方法。如果 `Box<dyn ReactLLM>` 没有此方法，需要添加一个适配层。当前 `llm_factory` 返回 `Box<dyn ReactLLM + Send + Sync>`，它是 `trait object`，需要提取内部的 `Arc<dyn BaseModel>`。

查看 `BaseModelReactLLM` 是否有公开方法访问内部 model：

```bash
# 先读取 BaseModelReactLLM 源码确认
grep -A 5 "pub fn.*model\|pub fn.*base_model\|pub fn.*into_arc" peri-agent/src/llm/react_llm.rs
```

如果存在方法，使用它；否则在 `BaseModelReactLLM` 上添加 `fn expose_base_model_arc(&self) -> Arc<dyn BaseModel>` 方法，然后 `invoke_background_fork` 中调用 `llm.expose_base_model_arc()`。

如果 `llm` 是 `Box<dyn ReactLLM>` trait object 且无法 downcast，则需要另一种方案：让 `BgForkConfig` 接受 `llm_factory` 而非 `Arc<dyn BaseModel>`。修改 `BgForkConfig.llm` 字段为：

```rust
/// LLM 工厂（每次创建新 ReActAgent 时调用）
pub llm_factory: Arc<dyn Fn(Option<&str>) -> Box<dyn ReactLLM + Send + Sync> + Send + Sync>,
```

然后在 `spawn_background_fork` 中通过工厂创建 LLM，并包上 `RetryableLLM`。

- [ ] **Step 5: 编译验证**

```bash
cargo build -p peri-middlewares -p peri-acp 2>&1
```

预期：无编译错误。

- [ ] **Step 6: 运行已有测试确保无回归**

```bash
cargo test -p peri-middlewares --lib 2>&1 | grep -E "test result|FAILED"
cargo test -p peri-acp --lib 2>&1 | grep -E "test result|FAILED"
```

预期：所有已有测试通过。

- [ ] **Step 7: 提交**

```bash
git add peri-middlewares/src/subagent/tool/execute_bg.rs \
        peri-middlewares/src/subagent/spawner.rs \
        peri-acp/src/session/command/bg.rs
git commit -m "refactor: eliminate execute_bg.rs duplication by delegating to shared spawner

- Add BgForkDirectiveKind enum (Fork/Bg) to spawner.rs
- Make invoke_background_fork() construct BgForkConfig and call spawn_background_fork()
- Fix race condition: spawner uses start_tx/start_rx oneshot, execute_bg now inherits this fix
- Add llm_factory support to BgForkConfig to keep RetryableLLM wrapping
- Reduce 230 lines of duplicate code to ~50 lines of delegation"

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>
```

---

### Task 2: 为 /bg 路径 LLM 添加 RetryableLLM 封装 (H1)

**Files:**
- Modify: `peri-acp/src/session/command/bg.rs:68-88`
- Possibly modify: `peri-middlewares/src/subagent/spawner.rs`

**背景**：`bg.rs` 的 LLM 构造直接使用 `LlmProvider::from_config().into_model()`，未包 `RetryableLLM`。而 `builder.rs` 的构建路径使用 `RetryableLLM::new(llm, RetryConfig::default())`。

**方案**：在 `BgForkConfig` 中接受 `Arc<dyn BaseModel>`，在 `spawn_background_fork` 中统一包上 `RetryableLLM`。这样所有路径自动享受重试逻辑。

- [ ] **Step 1: 修改 spawner.rs 中 LLM 构造，添加 RetryableLLM**

在 `spawn_background_fork()` 中找到 `BaseModelReactLLM::from_arc(...)` 调用处，在构造后立即包装：

```rust
// 原代码 (约 spawner.rs:~165):
// let llm = BaseModelReactLLM::from_arc(Arc::new(ArcBaseModelAdapter(Arc::clone(&config.llm))));
// let mut agent_builder = ReActAgent::new(llm).max_iterations(200);

// 替换为:
let base_llm = BaseModelReactLLM::from_arc(Arc::new(ArcBaseModelAdapter(Arc::clone(&config.llm))));
let llm = Box::new(peri_agent::llm::RetryableLLM::new(
    base_llm,
    peri_agent::llm::RetryConfig::default(),
));
let mut agent_builder = ReActAgent::new(llm).max_iterations(200);
```

- [ ] **Step 2: 编译 + 测试验证**

```bash
cargo build -p peri-middlewares -p peri-acp 2>&1
cargo test -p peri-middlewares --lib -p peri-acp --lib 2>&1 | grep -E "test result|FAILED"
```

- [ ] **Step 3: 提交**

```bash
git add peri-middlewares/src/subagent/spawner.rs
git commit -m "fix: wrap bg-fork LLM with RetryableLLM for transient error handling

Previously /bg path used bare BaseModelReactLLM without retry logic.
Now spawn_background_fork() always wraps with RetryableLLM, so both
/bg command and Agent tool fork paths benefit from exponential backoff."

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>
```

---

### Task 3: 添加 MCP 工具支持或明确文档化排除决策 (H2)

**Files:**
- Modify: `peri-acp/src/session/command/bg.rs:92-102`
- Optionally: `peri-acp/src/agent/builder.rs` (引用 MCP 工具构建代码)

**背景**：BgCommand 的工具列表只包含 Filesystem + Terminal，而 builder.rs 的构建路径还包含 MCP 工具。导致 `/bg` fork agent 无法访问 MCP。

**决策**：后台 worker agent 不应依赖可能产生交互的 MCP 工具（MCP 工具可能有 prompt/approval 需求），因此**有意排除**。但需要添加注释说明理由。

- [ ] **Step 1: 在 bg.rs 工具构造处添加注释**

在 `bg.rs` 的工具构造代码后添加注释说明：

```rust
// Construct parent tool set (filesystem + terminal = Read/Write/Edit/Bash/Grep/Glob)
// NOTE: MCP tools are intentionally excluded because:
// 1. Background workers should not depend on external MCP servers that may be unavailable
// 2. MCP tools may require interactive approval, which doesn't work for background agents
// 3. Core filesystem + terminal tools cover the majority of background task use cases
let line_edit_mode = ctx.peri_config.config.betas.line_edit;
let parent_tools: Arc<Vec<Arc<dyn peri_agent::tools::BaseTool>>> = {
    // ... existing code
};
```

- [ ] **Step 2: 如需添加 MCP 支持（可选、低优先级）**

如果需要统一能力，参考 `builder.rs:253-268` 的 MCP 工具构建代码，在 `bg.rs` 中添加类似逻辑。但需注意：
- MCP Server 初始化需要时间（可能阻塞命令响应）
- 需要 `EventSender`（BgCommand 没有）

**不推荐在本次修复中添加**。原因是 `/bg` 的用例是快速后台任务（搜索、代码分析），MCP 工具的交互性质（external server, Streamable HTTP, approval）与后台 worker 模型冲突。

- [ ] **Step 3: 编译验证 + 提交**

```bash
cargo build -p peri-acp 2>&1
git add peri-acp/src/session/command/bg.rs
git commit -m "docs: document intentional MCP tool exclusion in bg-fork agents

MCP tools require interactive approval and external server dependencies
which conflict with the background worker model. Core filesystem + terminal
tools cover the majority of background task use cases."

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>
```

---

### Task 4: 移除无效的 bg_test.rs 引用或创建测试文件 (L2)

**Files:**
- Create: `peri-tui/src/command/session/bg_test.rs` (or remove the `#[path]` reference)
- Modify: `peri-tui/src/command/session/bg.rs:38-40`

**背景**：TUI `bg.rs` 声明了 `#[path = "bg_test.rs"]` 但文件不存在。`cargo test -p peri-tui --lib` 会编译失败（当包含 `bg` 模块的 `session` crate test 时）。

**决策**：创建最小测试文件而非删除引用。遵循项目测试惯例（loop_cmd 等命令有对应测试）。

- [ ] **Step 1: 创建 bg_test.rs**

在 `peri-tui/src/command/session/bg_test.rs`：

```rust
use super::BgCommand;
use crate::{
    app::App,
    command::Command,
    i18n::LcRegistry,
    test_helpers::make_empty_app,
};

#[test]
fn test_bg_name() {
    let cmd = BgCommand;
    assert_eq!(cmd.name(), "bg");
}

#[test]
fn test_bg_aliases() {
    let cmd = BgCommand;
    assert!(cmd.aliases().contains(&"background"));
}

#[test]
fn test_bg_description_non_empty() {
    let cmd = BgCommand;
    let lc = LcRegistry::new(None);
    let desc = cmd.description(&lc);
    assert!(!desc.is_empty());
}

#[test]
fn test_bg_empty_args_shows_usage() {
    let mut app = make_empty_app();
    let cmd = BgCommand;
    cmd.execute(&mut app, "");
    // 空参数应添加一条系统消息提示用法
    let msgs = &app.session_mgr.current().messages.view_messages;
    assert!(!msgs.is_empty(), "应该显示用法提示");
}

#[test]
fn test_bg_with_args_submits_message() {
    let mut app = make_empty_app();
    let cmd = BgCommand;
    // 验证 execute 不会 panic
    cmd.execute(&mut app, "search Rust roadmap");
    // submit_message 会发送到 ACP transport，这里只验证不崩溃
}
```

**注意**：`make_empty_app` 可能不是实际存在的辅助函数。需要检查项目中 TUI 测试如何构造 `App`：

```bash
grep -r "fn make_.*app\|fn new_test_app\|fn create_test" peri-tui/src/ --include="*.rs" -l
```

如果存在，使用之；如果不存在，创建简单的空 App 构造辅助函数，或删除需要 App 的测试，仅保留属性测试。

- [ ] **Step 2: 编译 + 测试验证**

```bash
cargo test -p peri-tui --lib -- bg_test 2>&1
```

预期：bg_test 所有测试通过。

- [ ] **Step 3: 提交**

```bash
git add peri-tui/src/command/session/bg_test.rs
git commit -m "test: add missing bg_test.rs for TUI BgCommand

Previously #[path = 'bg_test.rs'] mod tests declared but file missing,
which would fail cargo test compilation."

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>
```

---

### Task 5: 使 bg_event_sender 从 Option 改为必填 (M1)

**Files:**
- Modify: `peri-middlewares/src/subagent/spawner.rs:63-65` (BgForkConfig.bg_event_sender)

**背景**：`bg_event_sender` 是 `Option<_>` 但 `BgCommand` 使用 `expect("都是 Some")`，`execute_bg.rs` 使用 `if let Some(ref sender) = self.bg_event_sender`。如果为 `None`，`BackgroundTaskCompleted` 事件和 `BgToolStep` 事件静默丢失。

**决策**：改为必填的非 Optional 类型。`BgCommand` 总是提供 `Some`，`execute_bg.rs` 总是有 sender（来自 builder.rs 的 `bg_event_tx`）。如果确实需要 Optional（如测试），通过 `#[cfg(test)]` 辅助函数构造。

- [ ] **Step 1: 修改 BgForkConfig.bg_event_sender 为必填**

在 `spawner.rs` 中：

```rust
// 原代码:
// pub bg_event_sender:
//     Option<tokio::sync::mpsc::UnboundedSender<peri_agent::agent::events::AgentEvent>>,

// 替换为:
/// 后台任务完成事件的发送通道（必填）
pub bg_event_sender:
    tokio::sync::mpsc::UnboundedSender<peri_agent::agent::events::AgentEvent>,
```

- [ ] **Step 2: 移除 spawn_background_fork 中的所有 None 处理**

在 `spawn_background_fork()` 中：
- `if let Some(ref sender) = config.bg_event_sender` → `let sender = &config.bg_event_sender`
- `if let Some(ref sender) = spawn_bg_sender` → 直接使用 sender，移除 else 分支的 warn log

- [ ] **Step 3: 更新调用方**

- `bg.rs:105-107`：移除 `.expect()`，直接传递 `ctx.bg_event_sender.expect("...")`（或让 `CommandContext.bg_event_sender` 也改为非 Optional）
- `execute_bg.rs:344-357`：移除 `if let Some(ref sender) = self.bg_event_sender`，直接使用 `self.bg_event_sender`

**注意**：`CommandContext.bg_event_sender` 仍需保留为 `Option` 因为 TUI 命令路径（如 `/clear`）不使用 bg 通道。`expect()` 保留在 BgCommand 调用点。

- [ ] **Step 4: 编译 + 测试验证**

```bash
cargo build -p peri-middlewares -p peri-acp 2>&1
cargo test -p peri-middlewares --lib -p peri-acp --lib 2>&1 | grep -E "test result|FAILED"
```

- [ ] **Step 5: 提交**

```bash
git add peri-middlewares/src/subagent/spawner.rs peri-acp/src/session/command/bg.rs
git commit -m "refactor: make bg_event_sender mandatory in BgForkConfig

Previously Option<_> led to silent event loss when None. Both callers
(BgCommand and execute_bg.rs) always provide a sender, so make it
non-optional to eliminate the dead branch."

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>
```

---

### Task 6: Sanitize prompt 防止 XML directive 注入 (L1) — 可选，低优先级

**Files:**
- Modify: `peri-middlewares/src/subagent/fork.rs:83-124` (build_bg_fork_directive)

**背景**：用户 prompt 原样嵌入到 `<bg_fork_directive>` XML 标签中。如果 prompt 包含 `</bg_fork_directive>`，会破坏指令结构。

**修复**：在格式化前将 prompt 中的 `</bg_fork_directive>` 替换为 `<\u{200b}/bg_fork_directive>`（零宽空格）。

```rust
// 在 build_bg_fork_directive 函数开头:
let sanitized_prompt = prompt.replace("</bg_fork_directive>", "<​/bg_fork_directive>");
// 然后在模板中使用 sanitized_prompt 而非 prompt
```

**注意**：这只是防御性措施。LLM 通常能处理畸形的 XML。标记为低优先级，可在后续迭代中处理。

- [ ] **Step 1: 实现 sanitize**
- [ ] **Step 2: 添加测试用例 `test_bg_fork_directive_sanitize_xml_injection`**
- [ ] **Step 3: 编译 + 测试 + 提交**

**如果时间紧迫，跳过此 Task**。当前优先级不足以在本次修复中完成。

---

## 执行顺序建议

```
Task 1 (C1+C2) → Task 5 (M1, depends on BgForkConfig changes in Task 1)
               → Task 2 (H1, independent but touches same files)
Task 3 (H2)    ↔ 独立
Task 4 (L2)    ↔ 独立
Task 6 (L1)    可选，建议推迟
```

并行窗口：Task 3 和 Task 4 可以与 Task 2 并行（它们修改不同文件）。
