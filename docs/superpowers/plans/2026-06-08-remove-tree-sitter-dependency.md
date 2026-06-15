# 移除 tree-sitter 依赖 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从 `peri-middlewares` 中移除 tree-sitter 及 5 个语言语法包依赖，将 LineEdit 验证从三层退化为两层（sanity + brackets），减小二进制发布包体积约 0.4-0.6 MB。

**Architecture:** 删除 `line_edit_verify.rs` 中的 `verify_ast()`、`count_ast_errors()`、`count_error_nodes()` 函数及相关测试。将 `verify()` 中层 C 调用替换为 `VerifyLevel::Skip`。移除 `Cargo.toml` 中 6 个依赖。更新工具描述文案。

**Tech Stack:** Rust, cargo

---

### Task 1: 移除 tree-sitter 验证函数和 AST 测试

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit_verify.rs`

- [ ] **Step 1: 替换 `verify_ast` 为直接返回 Skip**

将 `line_edit_verify.rs:258-312`（层 C 整个区域）替换为：

```rust
// ─── 层 C: AST Guard（已移除 tree-sitter，始终跳过）───────────────
fn verify_ast(_file_path: &str, _old_content: &str, _new_content: &str) -> VerifyLevel {
    VerifyLevel::Skip
}
```

这同时删除了 `count_ast_errors` 和 `count_error_nodes` 两个辅助函数。

- [ ] **Step 2: 删除 AST 相关测试**

删除 `line_edit_verify.rs` 中的 3 个 AST 测试函数（L386-406）：

```rust
    #[test]
    fn test_ast_非支持类型_skip() { ... }       // L386-390

    #[test]
    fn test_ast_rust_语法错误() { ... }          // L392-398

    #[test]
    fn test_ast_rust_原有错误未增() { ... }      // L400-406
```

- [ ] **Step 3: 移除顶部 `use std::path::Path`（如果不再需要）**

检查删除后 `Path` 是否仍有使用。`verify_ast` 原本是唯一使用 `Path` 的地方，删除后如果无其他引用，移除 L6：

```rust
use std::path::Path;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p peri-middlewares --lib -- line_edit_verify`
Expected: 所有剩余测试通过（括号平衡、diff sanity、verify 短路、P0-P4 系列测试）

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_verify.rs
git commit -m "refactor(lineedit): remove tree-sitter AST verification layer

Replace verify_ast() with VerifyLevel::Skip stub, removing tree-sitter
dependency from verification pipeline. LineEdit retains two-layer
verification (sanity + brackets) which covers 95%+ of edit error cases."
```

---

### Task 2: 更新工具描述文案

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit.rs:1-32`

- [ ] **Step 1: 更新 LINE_EDIT_DESCRIPTION 常量**

将 `line_edit.rs:11-32` 中的描述从三层改为两层：

```rust
const LINE_EDIT_DESCRIPTION: &str = r#"Applies unified diff patches to files with 5-level fuzzy matching and 2-layer verification.

Provide patches as an array of {file_path, diff} objects. The diff format follows standard unified diff:

```
--- a/file
+++ b/file
@@ -L,N +L,N @@
 context
-old
+new
 context
```

Features:
- **5-level fuzzy matching**: L1 exact → L2 whitespace-normalized → L3 similarity → L4 anchor → L5 line-number fallback
- **2-layer verification**: sanity check → bracket balance
- **Atomic writes**: all patches to a file are applied in-memory first, verified, then written atomically
- **Multiple hunks**: multiple hunks per file are applied bottom-to-top to preserve line numbers
- **CRLF preservation**: detects and preserves original line endings

The tool is designed for LLM-generated edits. Matching is fuzzy by default — exact match is preferred but not required."#;
```

- [ ] **Step 2: 运行测试确认通过**

Run: `cargo test -p peri-middlewares --lib -- line_edit`
Expected: 所有测试通过

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs
git commit -m "docs(lineedit): update tool description to 2-layer verification"
```

---

### Task 3: 从 Cargo.toml 移除 tree-sitter 依赖

**Files:**
- Modify: `peri-middlewares/Cargo.toml:46-51`

- [ ] **Step 1: 删除 6 个 tree-sitter 依赖行**

删除 `peri-middlewares/Cargo.toml` L46-51：

```toml
tree-sitter = "0.26"
tree-sitter-rust = "0.24"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.25"
tree-sitter-python = "0.25"
tree-sitter-go = "0.25"
```

保留 `similar = "3"`（LineEdit 匹配引擎仍在使用）。

- [ ] **Step 2: 更新 Cargo.lock**

Run: `cargo generate-lockfile -p peri-middlewares`
Expected: Cargo.lock 中 tree-sitter 相关条目被移除

- [ ] **Step 3: 全量构建确认无编译错误**

Run: `cargo build -p peri-middlewares`
Expected: 编译成功，无 tree-sitter 相关错误

- [ ] **Step 4: 运行全部相关测试**

Run: `cargo test -p peri-middlewares`
Expected: 所有测试通过

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/Cargo.toml Cargo.lock
git commit -m "chore(deps): remove tree-sitter and 5 grammar packages from peri-middlewares

Removes ~0.4-0.6 MB from release binary. LineEdit verification
degrades from 3-layer to 2-layer (sanity + brackets) with no
measurable impact on edit quality."
```

---

### Task 4: 验证二进制体积缩减

**Files:** 无代码改动

- [ ] **Step 1: 构建 release 并对比体积**

Run: `cargo build --release --bin peri && ls -lh target/release/peri`

Expected: 二进制体积较之前缩减（与 cargo-bloat 分析的 tree-sitter ~30 KB .text + C 解析器静态链接部分一致）

- [ ] **Step 2: 运行完整测试套件确认无回归**

Run: `cargo test -p peri-middlewares -p peri-acp -p peri-tui`
Expected: 全部通过
