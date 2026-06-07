# Write 工具 Append 模式实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Write 工具新增可选 `append` 参数，支持 LLM 只传增量内容追加到文件末尾，降低大文件写入的上下文消耗。

**Architecture:** 在现有 `WriteFileTool::invoke` 中分支处理：`append=true` 时用 `std::fs::OpenOptions::new().create(true).append(true)` 直接追加写入，不走原子写入路径（POSIX O_APPEND 保证单次 write 的原子性）。`append=false`（默认）保持现有原子写入逻辑不变。

**Tech Stack:** Rust, `std::fs::OpenOptions`, 现有 `resolve_path` 工具函数

---

## 文件结构

| 文件 | 职责 | 操作 |
|------|------|------|
| `peri-middlewares/src/tools/filesystem/write.rs` | Write 工具实现 | 修改 |
| `peri-middlewares/src/tools/filesystem/write_test.rs` | Write 工具测试 | 修改 |

### 不修改的文件（已知影响，留给后续 issue）

- `peri-acp/src/session/command/rewind.rs` — append 的 rewind 需要记录追加前的文件大小，属于独立 issue
- `peri-middlewares/src/attribution/mod.rs` — append 的 attribution 追踪（before_tool 读旧大小 → after_tool 读新大小），属于独立 issue
- `peri-tui/src/ui/message_render.rs` — append 的 inline diff 显示，属于独立 issue

---

### Task 1: 更新工具参数 schema 和描述

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/write.rs`

- [ ] **Step 1: 在 `parameters()` 中新增 `append` 参数**

在 `write.rs:42-56` 的 `parameters()` 方法中，在 `content` 属性后新增 `append` 属性：

```rust
fn parameters(&self) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "file_path": {
                "type": "string",
                "description": "The absolute path to the file to write (must be absolute, not relative)"
            },
            "content": {
                "type": "string",
                "description": "The full content to write to the file"
            },
            "append": {
                "type": "boolean",
                "description": "If true, append content to the end of the file instead of overwriting. Use this for writing large files in chunks: first call Write without append to create the file with the initial content, then call Write with append=true to add more content. This avoids sending the entire file content in a single tool call, saving context window space.",
                "default": false
            }
        },
        "required": ["file_path", "content"]
    })
}
```

- [ ] **Step 2: 更新工具描述，引导 LLM 使用 append 模式**

替换 `WRITE_FILE_DESCRIPTION` 常量（`write.rs:6-18`）：

```rust
const WRITE_FILE_DESCRIPTION: &str = r#"Writes a file to the local filesystem.

Usage:
- This tool will overwrite the existing file if there is one at the provided path
- If this is an existing file, you MUST use the Read tool first to read the file's contents. This tool will fail if you did not read the file first
- ALWAYS prefer editing existing files in the codebase. DO NOT create new files unless explicitly required
- The file_path parameter must be an absolute path, not a relative path
- Parent directories are created automatically if they do not exist

Notes:
- Uses atomic write (write to temp file then rename) to prevent data loss on crash
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User
- Only use emojis if the User explicitly requests it. Avoid writing emojis to files unless asked
- For files longer than 200 lines, consider writing in chunks: use Write for the first chunk, then Write with append=true for subsequent chunks. This reduces context window consumption significantly"#;
```

- [ ] **Step 3: 运行测试验证描述和参数变更**

Run: `cargo test -p peri-middlewares --lib -- test_description_extended`
Expected: PASS（现有断言检查 `desc.contains("Usage:")` 和 `desc.contains("atomic write")` 仍然满足）

Run: `cargo test -p peri-middlewares --lib -- test_tool_name_is_Write`
Expected: PASS

---

### Task 2: 实现 append 写入逻辑

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/write.rs`

- [ ] **Step 1: 在 `invoke()` 中提取 `append` 参数并分支处理**

替换 `write.rs:58-100` 的 `invoke()` 方法体。在解析 `file_path` 和 `content` 之后，提取 `append` 参数并分支：

```rust
async fn invoke(
    &self,
    input: Value,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let file_path = input["file_path"]
        .as_str()
        .ok_or("The 'file_path' parameter is required for the Write tool.")?;
    let content = input["content"]
        .as_str()
        .ok_or("The 'content' parameter is required for the Write tool.")?;
    let append = input["append"].as_bool().unwrap_or(false);

    let resolved = resolve_path(&self.cwd, file_path);
    let line_count = content.lines().count();

    if let Some(parent) = resolved.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    if append {
        // 追加模式：使用 OpenOptions::append 直接追加
        // POSIX O_APPEND 保证单次 write syscall 的原子性
        // create(true) 确保文件不存在时自动创建
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&resolved)
            .map_err(|e| format!("Error opening file for append: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("Error appending to file: {e}"))?;

        // 读取追加后的文件总行数
        let total_lines = std::fs::read_to_string(&resolved)
            .map(|s| s.lines().count())
            .unwrap_or(line_count);

        let rel = resolved
            .strip_prefix(&self.cwd)
            .unwrap_or(&resolved)
            .display()
            .to_string();
        let lines_label = if line_count == 1 { "line" } else { "lines" };
        Ok(format!(
            "Appended {} {} to {} (file total: {} lines)",
            line_count, lines_label, rel, total_lines
        ))
    } else {
        // 覆写模式：原子写入（现有逻辑不变）
        let tmp_ext = format!("tmp.{}", uuid::Uuid::now_v7());
        let tmp_path = resolved.with_extension(tmp_ext);
        if let Err(e) = std::fs::write(&tmp_path, content) {
            return Err(format!("Error writing file: {e}").into());
        }
        match std::fs::rename(&tmp_path, &resolved) {
            Ok(_) => {
                let rel = resolved
                    .strip_prefix(&self.cwd)
                    .unwrap_or(&resolved)
                    .display()
                    .to_string();
                let lines_label = if line_count == 1 { "line" } else { "lines" };
                Ok(format!("Wrote {} {} {}", line_count, lines_label, rel))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                Err(format!("Error renaming temp file: {e}").into())
            }
        }
    }
}
```

- [ ] **Step 2: 运行现有测试确认不破坏覆写模式**

Run: `cargo test -p peri-middlewares --lib -- write`
Expected: 所有现有测试 PASS

---

### Task 3: 编写 append 模式测试

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/write_test.rs`

- [ ] **Step 1: 测试 append 追加到已有文件**

在 `write_test.rs` 末尾（第 122 行 `}` 之前）追加：

```rust

    #[tokio::test]
    async fn test_write_append_to_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        // 先写入初始内容
        std::fs::write(dir.path().join("log.txt"), "line1\n").unwrap();
        let tool = WriteFileTool::new(dir.path().to_str().unwrap());
        let result = tool
            .invoke(serde_json::json!({
                "file_path": "log.txt",
                "content": "line2\n",
                "append": true
            }))
            .await
            .unwrap();
        let content = std::fs::read_to_string(dir.path().join("log.txt")).unwrap();
        assert_eq!(content, "line1\nline2\n");
        assert!(
            result.contains("Appended 1 line"),
            "unexpected message: {result}"
        );
        assert!(
            result.contains("file total: 2 lines"),
            "应包含总行数: {result}"
        );
    }
```

- [ ] **Step 2: 运行测试验证追加到已有文件**

Run: `cargo test -p peri-middlewares --lib -- test_write_append_to_existing_file`
Expected: PASS

- [ ] **Step 3: 测试 append 创建新文件（文件不存在时）**

追加测试：

```rust

    #[tokio::test]
    async fn test_write_append_creates_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path().to_str().unwrap());
        tool.invoke(serde_json::json!({
            "file_path": "new_append.txt",
            "content": "first line\n",
            "append": true
        }))
        .await
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("new_append.txt")).unwrap();
        assert_eq!(content, "first line\n");
    }
```

- [ ] **Step 4: 运行测试验证 append 创建新文件**

Run: `cargo test -p peri-middlewares --lib -- test_write_append_creates_new_file`
Expected: PASS

- [ ] **Step 5: 测试 append 多行内容**

追加测试：

```rust

    #[tokio::test]
    async fn test_write_append_multiline() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "a\n").unwrap();
        let tool = WriteFileTool::new(dir.path().to_str().unwrap());
        let result = tool
            .invoke(serde_json::json!({
                "file_path": "f.txt",
                "content": "b\nc\nd\n",
                "append": true
            }))
            .await
            .unwrap();
        let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
        assert_eq!(content, "a\nb\nc\nd\n");
        assert!(
            result.contains("Appended 3 lines"),
            "unexpected message: {result}"
        );
        assert!(
            result.contains("file total: 4 lines"),
            "应包含总行数: {result}"
        );
    }
```

- [ ] **Step 6: 运行测试验证多行追加**

Run: `cargo test -p peri-middlewares --lib -- test_write_append_multiline`
Expected: PASS

- [ ] **Step 7: 测试 append=false 保持覆写行为（回归）**

追加测试：

```rust

    #[tokio::test]
    async fn test_write_append_false_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "old content").unwrap();
        let tool = WriteFileTool::new(dir.path().to_str().unwrap());
        tool.invoke(serde_json::json!({
            "file_path": "f.txt",
            "content": "new",
            "append": false
        }))
        .await
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
        assert_eq!(content, "new", "append=false 应覆写文件");
    }
```

- [ ] **Step 8: 运行测试验证 append=false 覆写行为**

Run: `cargo test -p peri-middlewares --lib -- test_write_append_false_overwrites`
Expected: PASS

- [ ] **Step 9: 测试连续多次 append 模拟分块写入**

追加测试：

```rust

    #[tokio::test]
    async fn test_write_append_sequential_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let tool = WriteFileTool::new(dir.path().to_str().unwrap());
        // 第一次 Write 创建文件
        tool.invoke(serde_json::json!({
            "file_path": "chunked.txt",
            "content": "chunk1\n"
        }))
        .await
        .unwrap();
        // 后续 append 追加
        tool.invoke(serde_json::json!({
            "file_path": "chunked.txt",
            "content": "chunk2\n",
            "append": true
        }))
        .await
        .unwrap();
        tool.invoke(serde_json::json!({
            "file_path": "chunked.txt",
            "content": "chunk3\n",
            "append": true
        }))
        .await
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("chunked.txt")).unwrap();
        assert_eq!(content, "chunk1\nchunk2\nchunk3\n");
    }
```

- [ ] **Step 10: 运行测试验证分块写入**

Run: `cargo test -p peri-middlewares --lib -- test_write_append_sequential_chunks`
Expected: PASS

- [ ] **Step 11: 运行全量 Write 测试确认无回归**

Run: `cargo test -p peri-middlewares --lib -- write`
Expected: 所有测试 PASS（原有 9 个 + 新增 6 个 = 15 个）

- [ ] **Step 12: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/write.rs peri-middlewares/src/tools/filesystem/write_test.rs
git commit -m "feat(write): add append mode for incremental file writing

Write tool now supports optional `append` parameter (default: false).
When append=true, content is appended to file end using OpenOptions::append,
without reading the existing file content. This reduces context window
consumption for large file writes (observed up to 71KB / 2367 lines).

Closes: spec/issues/2026-06-06-write-tool-append-mode.md"
```

---

### Task 4: 构建验证

**Files:** 无修改

- [ ] **Step 1: 运行 cargo check 确认编译通过**

Run: `cargo check -p peri-middlewares`
Expected: 编译成功，无 warning

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy -p peri-middlewares -- -D warnings`
Expected: 无 warning

- [ ] **Step 3: 运行全量测试确认无破坏**

Run: `cargo test -p peri-middlewares --lib`
Expected: 所有测试 PASS

---

## Self-Review

### 1. Spec 覆盖

| Issue 要求 | 对应 Task |
|------------|-----------|
| 新增 `append` 参数 | Task 1 Step 1 |
| `std::fs::OpenOptions::new().append(true)` 写入 | Task 2 Step 1 |
| tool_result 返回 `Appended N lines to <path> (file total: M lines)` | Task 2 Step 1 |
| 文件不存在时自动创建（`create(true).append(true)`） | Task 2 Step 1 + Task 3 Step 3 |
| 工具描述引导 LLM 使用 append 模式 | Task 1 Step 2 |

### 2. Placeholder 扫描

无 TBD/TODO/placeholder。

### 3. 类型一致性

- `input["append"].as_bool().unwrap_or(false)` — 与 JSON schema `"type": "boolean", "default": false` 一致
- 出参格式 `Appended {n} {lines_label} to {rel} (file total: {total} lines)` — 测试中已断言
- `use std::io::Write` 在 append 分支内导入 — 不影响外部

### 未覆盖的后续工作（独立 issue）

- **Rewind 回退 append**：需要在 `rewind.rs` 中记录追加前的文件偏移量，属于 rewind 功能增强
- **Append 的 attribution 追踪**：`attribution/mod.rs` 需要在 `before_tool` 时记录文件旧大小，在 `after_tool` 时计算差值
- **Append 的 inline diff**：`message_render.rs` 需要对 append 模式显示"追加 N 行"而非 full diff
