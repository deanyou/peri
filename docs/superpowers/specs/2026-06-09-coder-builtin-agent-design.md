# coder Built-in Agent Design

**日期**：2026-06-09
**状态**：Approved
**关联 Issue**：[spec/issues/2026-06-09-coder-builtin-agent.md](../../spec/issues/2026-06-09-coder-builtin-agent.md)

## 1. 问题陈述

当前 4 个内置 Agent 中，`general-purpose` 承担了约 92.3% 的代码实现类任务（实现/迁移/重构），但它被设计为全工具通用 Agent。两个核心问题：

1. **上下文浪费**：全工具集（含 WebSearch、WebFetch、Agent、AskUserQuestion）占用 system prompt 空间，但在实现场景中极少使用
2. **退化循环**：典型案例中，一个 general-purpose SubAgent 对同一 pattern 执行 562 次 Grep——上下文被挤出后忘记搜索结果，陷入重复搜索

### 调研数据

| 指标 | 数值 |
|------|------|
| coder 类任务占 general-purpose 总量 | 92.3% |
| 极端案例消息数 | 717 |
| 极端案例 Grep 重复 | 562 次（同一 pattern） |
| 成功案例 P50 | 52 条消息 |
| 成功案例 P95 | 153 条消息 |

## 2. 设计目标

创建一个专门为代码实现优化的内置 Agent 类型 `coder`：

- 缩减工具集至 7 个，移除 4 个不相关工具
- 通过 Memory Discipline 规则防止退化循环
- 继承父 Agent 模型（sonnet 级别），保证编辑精准度
- 迭代上限 200（P95 + 30% 余量）

## 3. 设计决策

### 3.1 工具集

| 保留 (7) | 移除 (4) | 移除理由 |
|----------|----------|----------|
| Read | ~~WebSearch~~ | 实现场景不需要网页搜索 |
| Grep | ~~WebFetch~~ | 不需要抓取网页 |
| Glob | ~~Agent~~ | 不需要启动子 Agent |
| Bash | ~~AskUserQuestion~~ | "做但不猜测"模式，不问用户 |
| LineEdit | | |
| Write | | |
| TodoWrite | | |

### 3.2 模型：sonnet（inherit）

不锁定模型，通过 `model: inherit` 跟随父 Agent。理由：
- 代码编辑对精准度要求高，haiku 可能不够
- explore 用 haiku 是因为它只读不写
- 与 general-purpose 策略一致

### 3.3 迭代上限：200

- P95 = 153，留约 30% 余量
- general-purpose 默认也是 200（`AgentOverrides` 中 `max_turns` 默认值）
- 200 轮足以覆盖绝大多数实现任务

### 3.4 行为模式：做但不猜测

- 收到任务直接执行，不提问
- 遇到阻塞（3 次搜索无果、编辑持续失败）停止并报告
- 不自行猜测不确定的内容

### 3.5 Body 结构策略：反模式驱动

body 以 "CRITICAL: Memory Discipline" 为核心差异化章节，直接针对调研发现的退化模式：

- **禁止重复搜索**：Grep 前检查上下文是否已有结果
- **禁止重复读取**：已读过的文件不重读
- **阻塞即停**：3 次搜索无果或编辑持续失败时报告

同时保留执行惯例（Read → Find → Plan → Edit → Verify → Report）提供工作流指引。

## 4. 实现

### 4.1 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `peri-middlewares/src/subagent/built-in/coder.md` | **新建** | Agent 定义文件 |
| `peri-middlewares/src/subagent/built_in_agents.rs` | **修改** | 注册 coder 到内置 Agent 列表 |

### 4.2 `coder.md` 完整内容

```markdown
---
name: coder
description: "Code implementation specialist. Handles file editing, code migration, module refactoring, and other pure implementation tasks. Use this agent when the user needs to write code, modify files, or move modules — not for architecture design or solution evaluation."
tools: Read, Grep, Glob, Bash, LineEdit, Write, TodoWrite
disallowedTools: Agent
model: inherit
max_turns: 200
---

You are a code implementation specialist. Your output is file changes. 
You don't design systems, you don't evaluate solutions, you don't debug 
complex issues — you implement what's been decided.

## Execution Routine

When given an implementation task:

1. **Read targets** — Read the files mentioned in the task to understand 
   current state. Don't read unrelated files.
2. **Find impact** — Use Grep/Glob to locate all code that needs changing 
   (imports, callers, tests). Be surgical, not exploratory.
3. **Track plan** — Use TodoWrite to list what changes go where. 
   One item per file, not per line.
4. **Edit** — Make changes with LineEdit (precise edits) or Write (new 
   files / large rewrites).
5. **Verify** — Run build, tests, or lint with Bash if available.
6. **Report** — What files changed and why.

## CRITICAL: Memory Discipline

These are the rules that prevent task degradation. Violating them is the 
most common failure mode:

- **Never re-search for information already in your context.** Before 
  running Grep, check if you already searched for that pattern. If the 
  results are in your message history, use them — don't search again.
- **Never re-read files you've already read** unless the file was 
  modified after your last read.
- **Stop and report if blocked.** If you can't find the right location 
  after 3 search attempts, or an edit keeps failing, stop and report 
  what you know. Don't guess, don't loop.

## Tool Guidelines

- **LineEdit**: Default choice for editing existing files. Make one 
  focused change per call.
- **Write**: Only for creating new files or replacing entire file 
  contents. Not for appending to existing files.
- **Grep**: For finding code patterns. Use exact strings when possible. 
  Run in parallel with other searches.
- **Bash**: For build, test, lint commands. Chain with && for dependent 
  commands. Don't use for file reading (use Read instead).
- **TodoWrite**: Track WHAT needs changing, not HOW. Maximum 7 items.

When you complete the task, respond with a concise report: what files 
changed, what was done, any issues encountered.
```

### 4.3 `built_in_agents.rs` 修改

```rust
// 第 29 行：数组长度 4 → 5
static BUILT_IN_AGENTS: [BuiltInAgent; 5] = [
    BuiltInAgent {
        agent_id: "explore",
        content: include_str!("built-in/explore.md"),
    },
    BuiltInAgent {
        agent_id: "general-purpose",
        content: include_str!("built-in/general-purpose.md"),
    },
    BuiltInAgent {
        agent_id: "plan",
        content: include_str!("built-in/plan.md"),
    },
    BuiltInAgent {
        agent_id: "verification",
        content: include_str!("built-in/verification.md"),
    },
    // 新增 coder
    BuiltInAgent {
        agent_id: "coder",
        content: include_str!("built-in/coder.md"),
    },
];
```

### 4.4 不需要修改的部分

- **fork.rs / define.rs / tool/mod.rs**：Agent 加载和工具过滤逻辑是通用的，基于 YAML frontmatter 动态处理，新增 agent 类型无需改动
- **scan_agents**：自动扫描内置列表，无需额外注册
- **project-level 覆盖**：用户可在 `.claude/agents/coder.md` 创建项目级覆盖，内置 coder 作为 fallback

## 5. 验证标准

1. **编译通过**：`cargo build -p peri-middlewares` 无报错
2. **Agent 列表出现 coder**：父 Agent 的 `Agent` 工具 schema 中 `subagent_type` 枚举应包含 `"coder"`
3. **工具过滤正确**：coder SubAgent 的工具集应为 7 个（不含 WebSearch/WebFetch/Agent/AskUserQuestion）
4. **功能验证**：用同一代码实现任务分别以 `general-purpose` 和 `coder` 执行，对比：
   - 消息总数（coder 应 ≤ general-purpose）
   - Grep 调用次数（coder 应显著减少）
   - 是否出现重复搜索同一 pattern（coder 不应出现）
5. **指标更新**：`bun run src/metrics/subagent_collab.ts` 的"内置 Agent 分类分析"中应出现 coder 类型

## 6. 与其他 Agent 的对比

| | explore | general-purpose | coder | plan | verification |
|---|---|---|---|---|---|
| 模型 | haiku | inherit | inherit | inherit | inherit |
| 工具数 | 继承-4 | 全量 | 7 | 继承-4 | 继承-3 |
| 核心能力 | 搜索 | 搜索+分析+执行 | **编辑+执行** | 规划 | 验证 |
| 写文件 | ❌ | ✅ | ✅ | ❌ | ❌ |
| Memory Discipline | ❌ | ❌ | ✅ | ❌ | ❌ |
| 迭代上限 | 默认 | 默认 | 200 | 默认 | 默认 |

## 7. 风险与限制

- **Memory Discipline 依赖 LLM 遵循指令**：无法在代码层面强制"禁止重复搜索"，依赖 system prompt 质量
- **200 轮迭代上限可能不足**：对于极端复杂任务（717 条消息的案例），coder 仍可能在达到上限前耗尽上下文。但这类任务本身就超出了纯实现的范畴，应回退到 general-purpose 或由父 Agent 更细粒度地拆分任务
- **Tool Guidelines 可能过时**：如果未来新增代码编辑工具（如 Replace、Patch），需要同步更新 body
