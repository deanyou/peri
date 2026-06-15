# coder Built-in Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `coder` built-in agent type specialized for code implementation tasks (file editing, code migration, module refactoring).

**Architecture:** Create a new `coder.md` agent definition file with YAML frontmatter (7 tools, inherit model, 200 max_turns) and Memory Discipline body rules. Register it in `built_in_agents.rs` at position 0 to maintain alphabetical sort order required by existing tests.

**Tech Stack:** Rust (peri-middlewares crate), `include_str!` compile-time embedding, YAML frontmatter parsing via `claude_agent_parser`.

---

### Task 1: Create coder Agent Definition File

**Files:**
- Create: `peri-middlewares/src/subagent/built-in/coder.md`

- [ ] **Step 1: Write coder.md**

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

---

### Task 2: Register coder in built_in_agents.rs

**Files:**
- Modify: `peri-middlewares/src/subagent/built_in_agents.rs:29,42-46`

**IMPORTANT:** `coder` sorts before `explore` alphabetically. Tests verify sorted order (`test_built_in_agent_ids_unique`). Must insert at position 0.

- [ ] **Step 1: Update array size from 4 to 5**

In `peri-middlewares/src/subagent/built_in_agents.rs:29`, change:

```rust
static BUILT_IN_AGENTS: [BuiltInAgent; 4] = [
```

to:

```rust
static BUILT_IN_AGENTS: [BuiltInAgent; 5] = [
```

- [ ] **Step 2: Insert coder entry at position 0 (before explore)**

Insert after line 29 and before the `explore` entry:

```rust
    BuiltInAgent {
        agent_id: "coder",
        content: include_str!("built-in/coder.md"),
    },
```

The final array should look like:

```rust
static BUILT_IN_AGENTS: [BuiltInAgent; 5] = [
    BuiltInAgent {
        agent_id: "coder",
        content: include_str!("built-in/coder.md"),
    },
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
];
```

---

### Task 3: Update Tests

**Files:**
- Modify: `peri-middlewares/src/subagent/built_in_agents_test.rs:35-38`

The `test_get_built_in_agent_found` test explicitly lists expected agents. The `test_all_built_in_agents_parseable` and `test_built_in_agent_ids_unique` work automatically with any array content — no changes needed there.

- [ ] **Step 1: Add coder to found-agent assertions**

In `peri-middlewares/src/subagent/built_in_agents_test.rs`, add after the existing assertions:

```rust
#[test]
fn test_get_built_in_agent_found() {
    assert!(get_built_in_agent("explore").is_some());
    assert!(get_built_in_agent("plan").is_some());
    assert!(get_built_in_agent("general-purpose").is_some());
    assert!(get_built_in_agent("verification").is_some());
    assert!(get_built_in_agent("coder").is_some());
}
```

- [ ] **Step 2: (Optional) Add coder-specific test for tool set**

Add a new test at the end of the file to verify coder has exactly 7 tools:

```rust
#[test]
fn test_coder_agent_tools() {
    let agent = get_built_in_agent("coder").unwrap();
    let parsed = parse_agent_file(agent.content).unwrap();
    let tools = parsed.tools();
    assert_eq!(tools.len(), 7, "Coder agent should have exactly 7 tools");
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("LineEdit")),
        "Coder agent should have LineEdit"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Write")),
        "Coder agent should have Write"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Grep")),
        "Coder agent should have Grep"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Read")),
        "Coder agent should have Read"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Glob")),
        "Coder agent should have Glob"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Bash")),
        "Coder agent should have Bash"
    );
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("TodoWrite")),
        "Coder agent should have TodoWrite"
    );
}
```

---

### Task 4: Build and Test

- [ ] **Step 1: Build peri-middlewares**

Run: `cargo build -p peri-middlewares`
Expected: Compilation success, no errors.

- [ ] **Step 2: Run built_in_agents tests**

Run: `cargo test -p peri-middlewares --lib -- built_in_agents`
Expected: All 5 tests PASS (existing 4 + 1 new coder-specific test)

Or if optional test not added: All 4 tests PASS, including:
- `test_all_built_in_agents_parseable` — coder.md YAML frontmatter parses successfully
- `test_built_in_agent_ids_unique` — coder sorts before explore
- `test_get_built_in_agent_found` — coder lookup returns Some

---

### Task 5: Commit

- [ ] **Step 1: Stage and commit**

```bash
git add peri-middlewares/src/subagent/built-in/coder.md \
        peri-middlewares/src/subagent/built_in_agents.rs \
        peri-middlewares/src/subagent/built_in_agents_test.rs
git commit -m "feat(subagent): add coder built-in agent for code implementation tasks

Add a new coder agent type specialized for pure code implementation:
file editing, code migration, module refactoring. Uses 7 tools (Read,
Grep, Glob, Bash, LineEdit, Write, TodoWrite), inherits parent model,
200 max turns. Includes Memory Discipline rules to prevent the context
loss → repeat-search degradation observed in general-purpose agents.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```
