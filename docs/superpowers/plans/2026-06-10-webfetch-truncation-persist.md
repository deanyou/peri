# WebFetch 截断落盘 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** WebFetch 截断时将完整内容写入临时文件，行为与 Bash/Grep/Glob 等工具一致。

**Architecture:** 直接在 `truncate_content()` 内调用已有的 `persist_truncated_output()`，截断提示后追加落盘提示。无新模块，无接口变化。

**Tech Stack:** Rust，`crate::tools::output_persist::persist_truncated_output`（已有）

---

### Task 1: 编写失败测试

**Files:**
- Create: `peri-middlewares/src/middleware/web_fetch_test.rs`
- Modify: `peri-middlewares/src/middleware/web_fetch.rs`（添加 test module 声明）

- [ ] **Step 1: 在 web_fetch.rs 末尾追加 test module 声明**

在 `peri-middlewares/src/middleware/web_fetch.rs` 末尾（第 168 行之后）添加：

```rust
#[cfg(test)]
mod web_fetch_test;
```

- [ ] **Step 2: 创建测试文件**

新建 `peri-middlewares/src/middleware/web_fetch_test.rs`，内容如下：

```rust
use super::*;
use std::fs;

#[test]
fn test_truncate_content_超限时触发落盘() {
    // 生成 MAX_CONTENT_LINES + 1 行内容
    let lines: Vec<String> = (0..=MAX_CONTENT_LINES).map(|i| format!("line {i}")).collect();
    let full_content = lines.join("\n");
    let result = truncate_content(&full_content, MAX_CONTENT_LINES);
    // 截断提示存在
    assert!(result.contains("内容已截断"), "应包含截断提示: {result}");
    // 落盘提示存在
    assert!(result.contains("Read"), "应包含 Read 工具提示: {result}");
    // 从提示提取路径并验证文件内容
    let prefix = "saved to ";
    let suffix = " — use Read";
    let path_start = result.find(prefix).expect("应包含 'saved to'") + prefix.len();
    let path_end = result[path_start..]
        .find(suffix)
        .map(|i| path_start + i)
        .unwrap_or(result.len());
    let path = &result[path_start..path_end];
    let saved = fs::read_to_string(path).expect("落盘文件应存在");
    assert_eq!(saved, full_content, "落盘内容应与原始内容完全一致");
    fs::remove_file(path).ok();
}

#[test]
fn test_truncate_content_未超限时不落盘() {
    let content = "line1\nline2\nline3";
    let result = truncate_content(content, MAX_CONTENT_LINES);
    assert_eq!(result, content, "未超限时应原样返回");
    assert!(!result.contains("Read"), "未超限时不应有落盘提示: {result}");
}
```

- [ ] **Step 3: 运行测试，确认失败**

```bash
cargo test -p peri-middlewares --lib -- middleware::web_fetch_test 2>&1 | tail -20
```

期望：`test_truncate_content_超限时触发落盘` **FAILED**（`result` 不含 "Read"），`test_truncate_content_未超限时不落盘` **PASSED**。

---

### Task 2: 实现落盘逻辑并更新工具描述

**Files:**
- Modify: `peri-middlewares/src/middleware/web_fetch.rs`

- [ ] **Step 1: 在文件顶部 use 块末尾添加 import**

在 `web_fetch.rs` 现有 `use` 语句之后添加：

```rust
use crate::tools::output_persist::persist_truncated_output;
```

完整 use 区域变为：

```rust
use async_trait::async_trait;
use peri_agent::tools::BaseTool;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::output_persist::persist_truncated_output;
use super::web_common::WEB_CREDIBILITY_WARNING;
```

- [ ] **Step 2: 修改 `truncate_content` 函数**

将现有函数：

```rust
fn truncate_content(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        content.to_string()
    } else {
        let truncated: String = lines[..max_lines].join("\n");
        format!("{truncated}\n[内容已截断，原始内容共 {} 行]", lines.len())
    }
}
```

替换为：

```rust
fn truncate_content(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        content.to_string()
    } else {
        let truncated: String = lines[..max_lines].join("\n");
        let persist_hint = persist_truncated_output(content);
        format!(
            "{truncated}\n[内容已截断，原始内容共 {} 行]{persist_hint}",
            lines.len()
        )
    }
}
```

- [ ] **Step 3: 更新工具描述，说明落盘行为**

将 `WEB_FETCH_DESCRIPTION` 中的：

```
- Results are truncated at 2000 lines
```

替换为：

```
- Results are truncated at 2000 lines; full content saved to a temp file when truncated
```

- [ ] **Step 4: 运行测试，确认通过**

```bash
cargo test -p peri-middlewares --lib -- middleware::web_fetch_test 2>&1 | tail -20
```

期望：两个测试均 **PASSED**。

- [ ] **Step 5: 运行 clippy 确认无警告**

```bash
cargo clippy -p peri-middlewares 2>&1 | grep -E "^error|^warning" | head -20
```

期望：无新增 error 或 warning。

- [ ] **Step 6: 提交**

```bash
git add peri-middlewares/src/middleware/web_fetch.rs \
        peri-middlewares/src/middleware/web_fetch_test.rs
git commit -m "fix(web-fetch): 截断时落盘完整内容，与其他工具行为一致

closes spec/issues/2026-06-10-webfetch-truncation-no-disk-persist.md

Co-Authored-By: claude-sonnet-4-6 <noreply@anthropic.com>"
```
