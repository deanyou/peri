# Remove LineEdit Beta Feature Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完全移除 LineEdit beta 特性，回退到标准 Edit 工具（old_string 模型），betas 系统保留为空框架。

**Architecture:** 删除 5 个 LineEdit 工具源文件，从 FilesystemMiddleware 中剥离 line_edit_mode 开关，移除配置/Beta 面板/Agent 构建器中所有相关引用。内置 coder agent 用 Edit 替换 LineEdit。

**Tech Stack:** Rust, workspace (peri-middlewares, peri-acp, peri-tui)

**关联 Issue:** `spec/issues/2026-06-09-remove-lineedit-beta-feature.md`

---

## 文件结构总览

| 操作 | 文件 | 职责 |
|------|------|------|
| 删除 | `peri-middlewares/src/tools/filesystem/line_edit.rs` | V3 Diff-Apply 主入口 |
| 删除 | `peri-middlewares/src/tools/filesystem/line_edit_diff.rs` | Diff 解析器 |
| 删除 | `peri-middlewares/src/tools/filesystem/line_edit_match.rs` | 5 级匹配引擎 |
| 删除 | `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` | 3 层验证 |
| 删除 | `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | 测试 |
| 删除 | `prompts/lineedit_stress_test.txt` | 压力测试样本 |
| 修改 | `peri-middlewares/src/tools/filesystem/mod.rs` | 移除模块声明和导出 |
| 修改 | `peri-middlewares/src/middleware/filesystem.rs` | 剥离 line_edit_mode |
| 修改 | `peri-middlewares/src/tool_search/core_tools.rs` | 移除 TOOL_LINE_EDIT |
| 修改 | `peri-acp/src/provider/config.rs` | 移除 BetasConfig.line_edit |
| 修改 | `peri-acp/src/agent/builder.rs` | 移除 beta 读取 |
| 修改 | `peri-acp/src/session/command/bg.rs` | 移除 beta 读取 |
| 修改 | `peri-tui/src/app/betas_panel.rs` | 清空 BETA_KEYS |
| 修改 | `peri-middlewares/src/subagent/built-in/coder.md` | LineEdit → Edit |
| 修改 | `peri-middlewares/src/subagent/built_in_agents_test.rs` | 更新断言 |
| 修改 | `CLAUDE.md` | 更新 beta 表格 |

---

### Task 1: 删除 LineEdit 工具源文件

**Files:**
- Delete: `peri-middlewares/src/tools/filesystem/line_edit.rs`
- Delete: `peri-middlewares/src/tools/filesystem/line_edit_diff.rs`
- Delete: `peri-middlewares/src/tools/filesystem/line_edit_match.rs`
- Delete: `peri-middlewares/src/tools/filesystem/line_edit_verify.rs`
- Delete: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`
- Delete: `prompts/lineedit_stress_test.txt`

- [ ] **Step 1: 删除文件**

```bash
rm peri-middlewares/src/tools/filesystem/line_edit.rs
rm peri-middlewares/src/tools/filesystem/line_edit_diff.rs
rm peri-middlewares/src/tools/filesystem/line_edit_match.rs
rm peri-middlewares/src/tools/filesystem/line_edit_verify.rs
rm peri-middlewares/src/tools/filesystem/line_edit_test.rs
rm prompts/lineedit_stress_test.txt
```

- [ ] **Step 2: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs \
        peri-middlewares/src/tools/filesystem/line_edit_diff.rs \
        peri-middlewares/src/tools/filesystem/line_edit_match.rs \
        peri-middlewares/src/tools/filesystem/line_edit_verify.rs \
        peri-middlewares/src/tools/filesystem/line_edit_test.rs \
        prompts/lineedit_stress_test.txt
git commit -m "chore: remove LineEdit tool source files

Delete all 5 LineEdit source files and stress test prompt.
Part of removing the LineEdit beta feature."
```

---

### Task 2: 更新 `tools/filesystem/mod.rs` 移除 line_edit 模块

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/mod.rs`

- [ ] **Step 1: 移除 line_edit 模块声明和 LineEditTool 导出**

当前文件（行 7-10, 18）：

```rust
pub mod line_edit;
pub mod line_edit_diff;
pub mod line_edit_match;
pub mod line_edit_verify;
```

和：

```rust
pub use line_edit::LineEditTool;
```

改为 —— 删除上述 5 行：

```rust
pub mod edit;
pub mod folder;
pub mod glob;
pub mod grep;
pub(crate) mod grep_args;
pub(crate) mod grep_format;
pub mod read;
pub mod write;

pub use edit::EditFileTool;
pub use folder::FolderOperationsTool;
pub use glob::GlobFilesTool;
pub use grep::GrepTool;
pub use read::ReadFileTool;
pub use write::WriteFileTool;

use std::path::{Path, PathBuf};
```

- [ ] **Step 2: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/mod.rs
git commit -m "chore: remove LineEdit module declarations and exports"
```

---

### Task 3: 简化 `FilesystemMiddleware`

**Files:**
- Modify: `peri-middlewares/src/middleware/filesystem.rs`

- [ ] **Step 1: 重写 filesystem.rs**

当前完整内容（78 行）替换为以下内容 —— 移除 `line_edit_mode` 字段、`with_line_edit_mode`、`build_tools_with_mode`、`tool_names_line_edit`、`LineEditTool` 导入：

```rust
use async_trait::async_trait;
use peri_agent::{agent::state::State, middleware::r#trait::Middleware, tools::BaseTool};

use crate::tools::{
    EditFileTool, FolderOperationsTool, GlobFilesTool, GrepTool, ReadFileTool,
    WriteFileTool,
};

pub struct FilesystemMiddleware;

impl FilesystemMiddleware {
    pub fn new() -> Self {
        Self
    }

    pub fn build_tools(cwd: &str) -> Vec<Box<dyn BaseTool>> {
        vec![
            Box::new(ReadFileTool::new(cwd)),
            Box::new(WriteFileTool::new(cwd)),
            Box::new(EditFileTool::new(cwd)),
            Box::new(GlobFilesTool::new(cwd)),
            Box::new(GrepTool::new(cwd)),
            Box::new(FolderOperationsTool::new(cwd)),
        ]
    }

    pub fn tool_names() -> Vec<&'static str> {
        vec!["Read", "Write", "Edit", "Glob", "Grep", "folder_operations"]
    }
}

impl Default for FilesystemMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<S: State> Middleware<S> for FilesystemMiddleware {
    fn collect_tools(&self, cwd: &str) -> Vec<Box<dyn BaseTool>> {
        Self::build_tools(cwd)
    }

    fn name(&self) -> &str {
        "FilesystemMiddleware"
    }
}
```

变更摘要：
- 移除 `LineEditTool` 导入
- 移除 `line_edit_mode: bool` 字段
- 移除 `with_line_edit_mode()` 方法
- 移除 `build_tools_with_mode()` 方法 → `build_tools()` 始终使用 `EditFileTool`
- 移除 `tool_names_line_edit()` 方法
- `collect_tools()` 调用 `Self::build_tools(cwd)` 替代 `Self::build_tools_with_mode(cwd, self.line_edit_mode)`

- [ ] **Step 2: Commit**

```bash
git add peri-middlewares/src/middleware/filesystem.rs
git commit -m "refactor: strip line_edit_mode from FilesystemMiddleware

Remove line_edit_mode toggle. FilesystemMiddleware always uses
EditFileTool now. Removes with_line_edit_mode, build_tools_with_mode,
and tool_names_line_edit."
```

---

### Task 4: 更新 `core_tools.rs` 移除 TOOL_LINE_EDIT

**Files:**
- Modify: `peri-middlewares/src/tool_search/core_tools.rs`

- [ ] **Step 1: 移除 TOOL_LINE_EDIT 常量**

删除第 21 行：
```rust
pub const TOOL_LINE_EDIT: &str = "LineEdit";
```

- [ ] **Step 2: 更新 CORE_TOOLS 集合**

从 `CORE_TOOLS` 中移除 `TOOL_LINE_EDIT`。当前第 41-64 行，删除第 47 行的 `TOOL_LINE_EDIT,`：

```rust
/// 核心工具白名单（始终发送给 LLM，共 12 个）
///
/// - 文件操作 (6): Read, Write, Edit, Glob, Grep, folder_operations
/// - 执行 (1): Bash
/// - Web (2): WebFetch, WebSearch
/// - 交互 (2): Agent, AskUserQuestion
/// - 管理 (1): TodoWrite
pub static CORE_TOOLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // 文件操作
        TOOL_READ,
        TOOL_WRITE,
        TOOL_EDIT,
        TOOL_GLOB,
        TOOL_GREP,
        TOOL_FOLDER_OPS,
        // 执行
        TOOL_BASH,
        // Web
        TOOL_WEBFETCH,
        TOOL_WEBSEARCH,
        // 交互
        TOOL_AGENT,
        TOOL_ASK_USER,
        // 管理
        TOOL_TODO,
    ]
    .into_iter()
    .collect()
});
```

变更摘要：
- 注释：`13 个` → `12 个`，`文件操作 (7)` → `(6)`，`Edit, LineEdit` → `Edit`
- 删除 `TOOL_LINE_EDIT,` 行
- 删除"注意：Edit、LineEdit 不会同时存在"说明（第 40 行）

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tool_search/core_tools.rs
git commit -m "chore: remove TOOL_LINE_EDIT from core tools"
```

---

### Task 5: 移除 `BetasConfig.line_edit` 字段

**Files:**
- Modify: `peri-acp/src/provider/config.rs`

- [ ] **Step 1: 清空 BetasConfig**

当前（行 104-109）：
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BetasConfig {
    /// 启用 line edit 基于行号的编辑模式
    #[serde(default)]
    pub line_edit: bool,
}
```

改为空结构体：
```rust
/// Beta 功能开关配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BetasConfig {}
```

- [ ] **Step 2: Commit**

```bash
git add peri-acp/src/provider/config.rs
git commit -m "chore: remove line_edit field from BetasConfig"
```

---

### Task 6: 更新 `agent/builder.rs` 移除 beta 读取

**Files:**
- Modify: `peri-acp/src/agent/builder.rs`

- [ ] **Step 1: 替换行 238-241**

当前（行 238-241）：
```rust
    let line_edit_mode = peri_config.config.betas.line_edit;
    let filesystem_middleware = FilesystemMiddleware::new().with_line_edit_mode(line_edit_mode);
    let mut parent_tools: Vec<Box<dyn peri_agent::tools::BaseTool>> =
        FilesystemMiddleware::build_tools_with_mode(&cwd, line_edit_mode);
```

替换为：
```rust
    let filesystem_middleware = FilesystemMiddleware::new();
    let mut parent_tools: Vec<Box<dyn peri_agent::tools::BaseTool>> =
        FilesystemMiddleware::build_tools(&cwd);
```

- [ ] **Step 2: Commit**

```bash
git add peri-acp/src/agent/builder.rs
git commit -m "chore: remove line_edit_mode from agent builder"
```

---

### Task 7: 更新 `session/command/bg.rs` 移除 beta 读取

**Files:**
- Modify: `peri-acp/src/session/command/bg.rs`

- [ ] **Step 1: 替换行 97-100**

当前（行 97-100）：
```rust
        let line_edit_mode = ctx.peri_config.config.betas.line_edit;
        let parent_tools: Arc<Vec<Arc<dyn peri_agent::tools::BaseTool>>> = {
            let mut tools: Vec<Box<dyn peri_agent::tools::BaseTool>> =
                FilesystemMiddleware::build_tools_with_mode(&ctx.cwd, line_edit_mode);
```

替换为：
```rust
        let parent_tools: Arc<Vec<Arc<dyn peri_agent::tools::BaseTool>>> = {
            let mut tools: Vec<Box<dyn peri_agent::tools::BaseTool>> =
                FilesystemMiddleware::build_tools(&ctx.cwd);
```

- [ ] **Step 2: Commit**

```bash
git add peri-acp/src/session/command/bg.rs
git commit -m "chore: remove line_edit_mode from bg command"
```

---

### Task 8: 更新 TUI betas 面板

**Files:**
- Modify: `peri-tui/src/app/betas_panel.rs`

- [ ] **Step 1: 清空 BETA_KEYS 并简化 from_config/apply_to_config**

当前第 22 行：
```rust
const BETA_KEYS: &[&str] = &["lineEdit"];
```

改为空数组：
```rust
/// Beta 功能开关键值
const BETA_KEYS: &[&str] = &[];
```

当前 `from_config`（行 35-54）和 `apply_to_config`（行 65-71）：

```rust
    pub fn from_config(cfg: &crate::config::PeriConfig) -> Self {
        let entries = BETA_KEYS
            .iter()
            .map(|&key| match key {
                "lineEdit" => BetaEntry {
                    key: key.to_string(),
                    label: "LineEdit".to_string(),
                    description: "基于行号的精确编辑模式".to_string(),
                    enabled: cfg.config.betas.line_edit,
                },
                _ => BetaEntry {
                    key: key.to_string(),
                    label: key.to_string(),
                    description: String::new(),
                    enabled: false,
                },
            })
            .collect();

        Self { entries, cursor: 0 }
    }

    pub fn apply_to_config(&self, cfg: &mut crate::config::PeriConfig) {
        for entry in &self.entries {
            if entry.key == "lineEdit" {
                cfg.config.betas.line_edit = entry.enabled;
            }
        }
    }
```

改为（保留 `from_config` 和 `apply_to_config` 作为空操作，保持 API 稳定）：

```rust
    pub fn from_config(_cfg: &crate::config::PeriConfig) -> Self {
        let entries = BETA_KEYS
            .iter()
            .map(|&key| BetaEntry {
                key: key.to_string(),
                label: key.to_string(),
                description: String::new(),
                enabled: false,
            })
            .collect();

        Self { entries, cursor: 0 }
    }

    pub fn apply_to_config(&self, _cfg: &mut crate::config::PeriConfig) {
        // 当前无活跃 beta 功能，无配置可应用
    }
```

- [ ] **Step 2: Commit**

```bash
git add peri-tui/src/app/betas_panel.rs
git commit -m "chore: clear BETA_KEYS after LineEdit removal"
```

---

### Task 9: 更新内置 coder agent 定义

**Files:**
- Modify: `peri-middlewares/src/subagent/built-in/coder.md`

- [ ] **Step 1: 替换 LineEdit → Edit**

当前第 4 行：
```yaml
tools: Read, Grep, Glob, Bash, LineEdit, Write, TodoWrite
```

改为：
```yaml
tools: Read, Grep, Glob, Bash, Edit, Write, TodoWrite
```

第 25 行：
```markdown
4. **Edit** — Make changes with LineEdit (precise edits) or Write (new 
```

改为：
```markdown
4. **Edit** — Make changes with Edit (precise edits) or Write (new 
```

第 46 行：
```markdown
- **LineEdit**: Default choice for editing existing files. Make one 
```

改为：
```markdown
- **Edit**: Default choice for editing existing files. Make one 
```

- [ ] **Step 2: Commit**

```bash
git add peri-middlewares/src/subagent/built-in/coder.md
git commit -m "chore: replace LineEdit with Edit in built-in coder agent"
```

---

### Task 10: 更新内置 agent 测试断言

**Files:**
- Modify: `peri-middlewares/src/subagent/built_in_agents_test.rs`

- [ ] **Step 1: 更新断言**

当前第 79-82 行：
```rust
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("LineEdit")),
        "Coder agent should have LineEdit"
    );
```

改为：
```rust
    assert!(
        tools.iter().any(|t| t.eq_ignore_ascii_case("Edit")),
        "Coder agent should have Edit"
    );
```

- [ ] **Step 2: Commit**

```bash
git add peri-middlewares/src/subagent/built_in_agents_test.rs
git commit -m "test: update coder agent test to expect Edit instead of LineEdit"
```

---

### Task 11: 更新 CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: 更新 Beta 功能开关章节**

当前（行 198-204）：
```markdown
## Beta 功能开关

`settings.json` → `config.betas` 控制 beta 功能。所有字段默认 `false`。

| 字段 | 说明 |
|------|------|
| `lineEdit` | 启用行号编辑模式——Edit 替换为 LineEdit（unified diff 输入、5 级匹配回退、3 层验证、原子性写入） |
```

改为：
```markdown
## Beta 功能开关

`settings.json` → `config.betas` 控制 beta 功能。所有字段默认 `false`。

当前无活跃 beta 功能。
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update beta section after LineEdit removal"
```

---

### Task 12: 编译和测试验证

**Files:** 无（验证步骤）

- [ ] **Step 1: 编译全 workspace**

```bash
cargo build 2>&1
```

Expected: 编译成功，无 `line_edit` 相关错误。

- [ ] **Step 2: 运行 peri-middlewares 测试**

```bash
cargo test -p peri-middlewares --lib 2>&1
```

Expected: 所有测试通过（含 `test_coder_agent_tools`）。

- [ ] **Step 3: 运行 peri-acp 测试**

```bash
cargo test -p peri-acp --lib 2>&1
```

Expected: 所有测试通过。

- [ ] **Step 4: 运行 peri-tui 测试**

```bash
cargo test -p peri-tui --lib 2>&1
```

Expected: 所有测试通过。

- [ ] **Step 5: 运行全量测试**

```bash
cargo test 2>&1
```

Expected: 所有 workspace 测试通过。

---

### Task 13: 终态验证

**Files:** 无（验证步骤）

- [ ] **Step 1: Grep 验证无残留引用**

```bash
grep -r "LineEdit" --include="*.rs" peri-middlewares/src/ peri-acp/src/ peri-tui/src/ || echo "No LineEdit references found"
grep -r "line_edit" --include="*.rs" peri-middlewares/src/ peri-acp/src/ peri-tui/src/ || echo "No line_edit references found"
grep -r "lineEdit" --include="*.rs" --include="*.md" peri-tui/src/app/betas_panel.rs || echo "No lineEdit in betas_panel.rs"
```

Expected: 以上 grep 均无匹配（或仅在注释/字符串中有意保留的引用）。

- [ ] **Step 2: 验证 betas 面板仍可正常打开**

启动 TUI 后执行 `/betas` 命令，面板应正常显示（无条目，显示"当前无活跃 beta 功能"或空列表）。

- [ ] **Step 3: Final commit（如有遗漏修复）**

仅当验证发现问题时有此步骤。

---

## 自审清单

1. **Spec 覆盖**：Issue `2026-06-09-remove-lineedit-beta-feature.md` 列出的 16 个文件全部覆盖 —— 5 删除 + 11 修改均已对应 Task
2. **Placeholder 扫描**：无 TBD/TODO，所有步骤包含实际代码或精确命令
3. **类型一致性**：`build_tools()` 签名在 task 3（定义）和 task 6/7（调用）一致
4. **不涉及的修改**（有意排除）：
   - `side-projects/agent-defect-analyzer/`：实验性分析工具，非核心代码，保持不动
   - `docs/superpowers/specs/` 和 `docs/superpowers/plans/`：历史设计文档，保留作为存档
   - `spec/issues/` 中已有 lineEdit 相关 issue：不追溯修改，`issue-verify` 后续处理
   - `spec/global/` 中 lineEdit 相关记录：不追溯修改
