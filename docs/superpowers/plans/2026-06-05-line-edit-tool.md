# LineEdit 工具实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现基于行号的 LineEdit 工具，替换 Edit（old_string）和 HashlineEdit（hash），通过 `betas.lineEdit` beta 开关控制。

**Architecture:** 新增 `peri-middlewares/src/tools/filesystem/line_edit.rs`，行号定位 + `start_word`/`end_word` 行内精度 + 多编辑从后往前应用。复用现有 `atomic_write` 和 `resolve_path`。注册逻辑复用 `FilesystemMiddleware` 的 beta 分支模式。

**Tech Stack:** Rust, tokio + async-trait, serde_json, tempfile（测试）

**Design Spec:** `docs/superpowers/specs/2026-06-05-line-edit-tool-design.md`

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `peri-middlewares/src/tools/filesystem/line_edit.rs` | 创建 | LineEdit 工具核心实现（结构体 + BaseTool impl + 编辑应用逻辑） |
| `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | 创建 | LineEdit 测试 |
| `peri-middlewares/src/tools/filesystem/mod.rs` | 修改 | 添加 `pub mod line_edit` + `pub use line_edit::LineEditTool` |
| `peri-middlewares/src/middleware/filesystem.rs` | 修改 | 添加 `line_edit_mode` 字段 + `build_tools_with_line_edit` 分支 |
| `peri-middlewares/src/tools/mod.rs` | 修改 | 添加 `LineEditTool` 导出 |
| `peri-middlewares/src/tool_search/core_tools.rs` | 修改 | 添加 `TOOL_LINE_EDIT` 常量 + 更新 CORE_TOOLS |
| `peri-acp/src/provider/config.rs` | 修改 | `BetasConfig` 添加 `line_edit: bool` 字段 |
| `peri-acp/src/agent/builder.rs` | 修改 | 读取 `betas.lineEdit` + 传递给 FilesystemMiddleware |
| `peri-tui/src/app/betas_panel.rs` | 修改 | `BETA_KEYS` 添加 `"lineEdit"` + 对应 BetaEntry |

**清理（Task 7）**：

| 文件 | 操作 | 说明 |
|------|------|------|
| `peri-middlewares/src/tools/hashline/` | 删除 | 整个目录（13 个文件） |
| `peri-middlewares/src/tools/filesystem/read.rs` | 修改 | 移除 `hashline_mode`/`snapshot_cache`/`with_hashline` + hashline 输出分支 |
| `peri-middlewares/src/middleware/filesystem.rs` | 修改 | 移除 `hashline_mode`/`snapshot_cache`/`with_hashline_mode` + 相关 import |
| `peri-middlewares/src/tools/mod.rs` | 修改 | 移除 `hashline` 模块导出 |
| `peri-tui/src/app/betas_panel.rs` | 修改 | 移除 `"hashline"` beta 条目 |
| `peri-acp/src/provider/config.rs` | 修改 | 移除 `hashline` 字段 |

---

### Task 1: LineEdit 核心结构体和单行替换

**Files:**
- Create: `peri-middlewares/src/tools/filesystem/line_edit.rs`
- Create: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

- [ ] **Step 1: 创建 line_edit.rs 骨架 + LineEditInput 结构体 + 单行替换测试**

创建 `peri-middlewares/src/tools/filesystem/line_edit.rs`：

```rust
use peri_agent::tools::BaseTool;
use serde::Deserialize;
use serde_json::Value;

use super::resolve_path;

const LINE_EDIT_DESCRIPTION: &str = r#"Performs precise line-based edits in files.

Usage:
- Uses line numbers from Read output to target edits — no content matching needed
- start_line and end_line refer to the line numbers shown in Read output (1-based)
- Use start_word/end_word to target specific positions within a line (optional)
- Supports multiple edits in a single call — they are applied bottom-to-top for stability
- insert: true inserts new lines before start_line without replacing anything
- Set new_string to empty string to delete lines

Parameters:
- file_path (required): absolute path to the file
- start_line (required): line number to start editing (from Read output)
- end_line (optional): line number to end editing, defaults to start_line
- start_word (optional): text within start_line to begin replacement at — must be unique in that line
- end_word (optional): text within end_line to end replacement at — must be unique in that line
- new_string (required): replacement text (empty string = delete)
- insert (optional): if true, insert new_string before start_line (no lines replaced)

Error handling:
- start_line exceeds file length: error with file line count
- start_word/end_word not found in target line: error with line content hint
- start_word/end_word matches multiple positions in line: error with match count, request longer word
- File not found: error
- end_line < start_line (non-insert): error"#;

/// 单个编辑操作
#[derive(Debug, Deserialize)]
pub struct EditEntry {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: Option<usize>,
    pub start_word: Option<String>,
    pub end_word: Option<String>,
    pub new_string: String,
    #[serde(default)]
    pub insert: bool,
}

/// LineEdit 工具 — 基于行号的精确编辑
pub struct LineEditTool {
    pub cwd: String,
}

impl LineEditTool {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self { cwd: cwd.into() }
    }
}

#[async_trait::async_trait]
impl BaseTool for LineEditTool {
    fn name(&self) -> &str {
        "LineEdit"
    }

    fn description(&self) -> &str {
        LINE_EDIT_DESCRIPTION
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Absolute path to the file to modify"
                            },
                            "start_line": {
                                "type": "integer",
                                "description": "Line number to start editing (from Read output)"
                            },
                            "end_line": {
                                "type": "integer",
                                "description": "Line number to end editing. Defaults to start_line if not provided"
                            },
                            "start_word": {
                                "type": "string",
                                "description": "Optional: text within start_line to begin replacement at. Must be unique in that line"
                            },
                            "end_word": {
                                "type": "string",
                                "description": "Optional: text within end_line to end replacement at. Must be unique in that line"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text. Empty string deletes the target range"
                            },
                            "insert": {
                                "type": "boolean",
                                "description": "If true, insert new_string before start_line without replacing any lines"
                            }
                        },
                        "required": ["file_path", "start_line", "new_string"]
                    }
                }
            },
            "required": ["edits"]
        })
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let edits: Vec<EditEntry> = serde_json::from_value(input["edits"].clone())
            .map_err(|e| format!("edits 参数解析失败: {}", e))?;

        if edits.is_empty() {
            return Err("edits 不能为空".into());
        }

        // 按文件分组
        let mut groups: std::collections::HashMap<String, Vec<&EditEntry>> =
            std::collections::HashMap::new();
        for edit in &edits {
            let resolved = resolve_path(&self.cwd, &edit.file_path);
            let key = resolved.to_string_lossy().to_string();
            groups.entry(key).or_default().push(edit);
        }

        let mut results: Vec<String> = Vec::new();

        for (file_key, file_edits) in &groups {
            // 按行号降序排列（从后往前）
            let mut sorted_edits: Vec<&&EditEntry> = file_edits.iter().collect();
            sorted_edits.sort_by(|a, b| b.start_line.cmp(&a.start_line));

            let content = match std::fs::read_to_string(file_key) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(format!("文件不存在: {}", file_key).into());
                }
                Err(e) => return Err(e.into()),
            };

            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            // 注意：文件末尾换行处理
            let trailing_newline = content.ends_with('\n');

            for edit in sorted_edits {
                match apply_single_edit(&mut lines, edit, &trailing_newline) {
                    Ok(msg) => results.push(msg),
                    Err(e) => results.push(format!("失败: {}", e)),
                }
            }

            // 原子写入
            let new_content = lines.join("\n")
                + if trailing_newline && !lines.is_empty() {
                    "\n"
                } else {
                    ""
                };
            atomic_write(std::path::Path::new(file_key), &new_content)?;
        }

        Ok(results.join("\n"))
    }
}

/// 应用单个编辑到行数组
fn apply_single_edit(
    lines: &mut Vec<String>,
    edit: &EditEntry,
    _trailing_newline: &bool,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let start_line = edit.start_line;
    let end_line = edit.end_line.unwrap_or(start_line);

    if edit.insert {
        return apply_insert(lines, start_line, &edit.new_string);
    }

    if start_line == 0 {
        return Err("start_line 必须 >= 1".into());
    }
    if end_line < start_line {
        return Err(format!(
            "end_line ({}) 不能小于 start_line ({})",
            end_line, start_line
        ));
    }
    if start_line > lines.len() {
        return Err(format!(
            "start_line {} 超出文件行数 (共 {} 行)",
            start_line,
            lines.len()
        ));
    }
    if end_line > lines.len() {
        return Err(format!(
            "end_line {} 超出文件行数 (共 {} 行)",
            end_line,
            lines.len()
        ));
    }

    // 转为 0-based 索引
    let start_idx = start_line - 1;
    let end_idx = end_line - 1;

    if edit.start_word.is_some() || edit.end_word.is_some() {
        return apply_word_edit(
            lines,
            start_idx,
            end_idx,
            edit.start_word.as_deref(),
            edit.end_word.as_deref(),
            &edit.new_string,
        );
    }

    // 整行替换
    let old_line_count = end_idx - start_idx + 1;
    let new_lines: Vec<&str> = edit.new_string.lines().collect();
    let diff = new_lines.len() as i64 - old_line_count as i64;

    lines.splice(start_idx..=end_idx, new_lines.iter().map(|s| s.to_string()));

    let desc = describe_diff(diff);
    Ok(format!(
        "{} 第 {}-{} 行 → {} 行",
        desc, start_line, end_line, new_lines.len()
    ))
}

/// 插入操作
fn apply_insert(
    lines: &mut Vec<String>,
    before_line: usize,
    new_string: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if before_line == 0 {
        return Err("insert 的 start_line 必须 >= 1".into());
    }
    if before_line > lines.len() + 1 {
        return Err(format!(
            "insert 位置 {} 超出范围 (文件共 {} 行，最大可插入到 {})",
            before_line,
            lines.len(),
            lines.len() + 1
        ));
    }

    let new_lines: Vec<&str> = new_string.lines().collect();
    let insert_idx = before_line - 1; // 在第 before_line 前插入 = 0-based 的 before_line-1

    for (i, line) in new_lines.iter().enumerate() {
        lines.insert(insert_idx + i, line.to_string());
    }

    Ok(format!(
        "插入 {} 行到第 {} 行前",
        new_lines.len(),
        before_line
    ))
}

/// 行内编辑（start_word / end_word）
fn apply_word_edit(
    lines: &mut Vec<String>,
    start_idx: usize,
    end_idx: usize,
    start_word: Option<&str>,
    end_word: Option<&str>,
    new_string: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // 处理 start_word
    let start_col = if let Some(word) = start_word {
        find_unique_word(&lines[start_idx], word)?
    } else {
        0
    };

    // 处理 end_word
    let end_col = if let Some(word) = end_word {
        let pos = find_unique_word(&lines[end_idx], word)?;
        pos + word.len()
    } else {
        lines[end_idx].len()
    };

    // 构建新内容
    let line_start = &lines[start_idx][..start_col];
    let line_end = &lines[end_idx][end_col..];

    let combined = format!("{}{}{}", line_start, new_string, line_end);

    // 替换范围为新内容
    let new_lines: Vec<String> = combined.lines().map(|s| s.to_string()).collect();
    lines.splice(start_idx..=end_idx, new_lines);

    Ok(format!(
        "行内替换 第 {}-{} 行 (start_word: {}, end_word: {})",
        start_idx + 1,
        end_idx + 1,
        start_word.unwrap_or("(行首)"),
        end_word.unwrap_or("(行尾)")
    ))
}

/// 在行内查找唯一匹配的 word 位置
fn find_unique_word(
    line: &str,
    word: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let matches: Vec<usize> = line.match_indices(word).map(|(i, _)| i).collect();

    match matches.len() {
        0 => Err(format!(
            "word '{}' 未在第 '{}' 行中找到",
            word,
            line.chars().take(80).collect::<String>()
        )
        .into()),
        1 => Ok(matches[0]),
        n => Err(format!(
            "word '{}' 在行内匹配了 {} 处，请提供更长的前缀使其唯一",
            word, n
        )
        .into()),
    }
}

fn describe_diff(diff: i64) -> &'static str {
    match diff.cmp(&0) {
        std::cmp::Ordering::Greater => "新增",
        std::cmp::Ordering::Less => "删除",
        std::cmp::Ordering::Equal => "替换",
    }
}

/// 原子写入：临时文件 + rename
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
    let tmp_ext = format!("tmp.{}", uuid::Uuid::now_v7());
    let tmp_path = path.with_extension(tmp_ext);
    std::fs::write(&tmp_path, content)?;
    match std::fs::rename(&tmp_path, path) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("line_edit_test.rs");
}
```

创建 `peri-middlewares/src/tools/filesystem/line_edit_test.rs`：

```rust
use super::*;

fn make_tool(dir: &tempfile::TempDir) -> LineEditTool {
    LineEditTool::new(dir.path().to_str().unwrap())
}

fn make_edit(
    file: &str,
    start_line: usize,
    new_string: &str,
) -> serde_json::Value {
    serde_json::json!({
        "file_path": file,
        "start_line": start_line,
        "new_string": new_string
    })
}

#[tokio::test]
async fn test_line_edit_替换单行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("f.txt", 2, "BBB")]
        }))
        .await
        .unwrap();
    assert!(result.contains("替换"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nBBB\nccc\n");
}

#[tokio::test]
async fn test_line_edit_替换多行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\nddd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 2,
                "end_line": 3,
                "new_string": "XXX"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("新增"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nXXX\nddd\n");
}

#[tokio::test]
async fn test_line_edit_删除行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [make_edit("f.txt", 2, "")]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nccc\n");
}

#[tokio::test]
async fn test_line_edit_插入() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 2,
                "new_string": "xxx\nyyy",
                "insert": true
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("插入"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nxxx\nyyy\nbbb\n");
}

#[tokio::test]
async fn test_line_edit_start_line超出范围() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("f.txt", 99, "xxx")]
        }))
        .await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("超出"), "应报超出范围: {err}");
}

#[tokio::test]
async fn test_line_edit_文件不存在() {
    let dir = tempfile::tempdir().unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("ghost.txt", 1, "xxx")]
        }))
        .await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("不存在"), "应报文件不存在: {err}");
}

#[tokio::test]
async fn test_line_edit_end_line小于start_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 3,
                "end_line": 1,
                "new_string": "xxx"
            }]
        }))
        .await;
    // best-effort: 结果中应包含失败信息
    let output = result.unwrap();
    assert!(output.contains("失败"), "应报告失败: {output}");
}
```

- [ ] **Step 2: 运行测试验证基本功能**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit::tests`
Expected: 所有测试 PASS

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "feat: add LineEdit tool with line-number based editing"
```

---

### Task 2: start_word / end_word 行内编辑

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit.rs`
- Modify: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

- [ ] **Step 1: 添加行内编辑测试**

在 `line_edit_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn test_line_edit_start_word行内替换() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("f.txt"),
        "pub async fn handle(&self, req: Request, config: &Config) -> Result {\n",
    )
    .unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "req:",
                "end_word": "Config)",
                "new_string": "input: Input, opts: Options"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("行内替换"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(
        content,
        "pub async fn handle(&self, input: Input, opts: Options) -> Result {\n"
    );
}

#[tokio::test]
async fn test_line_edit_start_word不匹配报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hello world\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "missing",
                "new_string": "xxx"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "应报告失败: {result}");
    assert!(result.contains("未在"), "应报告未找到: {result}");
}

#[tokio::test]
async fn test_line_edit_start_word多处匹配报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "foo and foo and foo\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "foo",
                "new_string": "bar"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "应报告失败: {result}");
    assert!(result.contains("3 处"), "应报告匹配次数: {result}");
}

#[tokio::test]
async fn test_line_edit_end_word定位() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa [remove me] bbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "[remove",
                "end_word": "me]",
                "new_string": "kept"
            }]
        }))
        .await
        .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa kept bbb\n");
}

#[tokio::test]
async fn test_line_edit_insert忽略start_word() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    // insert 模式下 start_word/end_word 应被忽略
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "ignored",
                "new_string": "xxx",
                "insert": true
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("插入"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "xxx\naaa\nbbb\n");
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit::tests`
Expected: 所有测试 PASS（start_word/end_word 逻辑已在 Task 1 的 `apply_word_edit` 中实现）

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "test: add start_word/end_word tests for LineEdit"
```

---

### Task 3: 多编辑从后往前应用

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

- [ ] **Step 1: 添加多编辑测试**

在 `line_edit_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn test_line_edit_同文件多编辑从后往前() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "f.txt", "start_line": 2, "new_string": "BBB"},
                {"file_path": "f.txt", "start_line": 4, "new_string": "DDD"}
            ]
        }))
        .await
        .unwrap();
    // 两个编辑都应成功
    assert!(!result.contains("失败"), "不应有失败: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "a\nBBB\nc\nDDD\ne\n");
}

#[tokio::test]
async fn test_line_edit_多编辑前增后减行号稳定() {
    // 第一个编辑增加行数，第二个编辑在更前面
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                // 先应用（从后往前）：第 3 行 → 3 行（X\nY\nZ），增加了 2 行
                {"file_path": "f.txt", "start_line": 3, "new_string": "X\nY\nZ"},
                // 再应用：第 1 行 → AAA（行号不受影响，因为从后往前）
                {"file_path": "f.txt", "start_line": 1, "new_string": "AAA"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("失败"), "不应有失败: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "AAA\nb\nX\nY\nZ\nd\n");
}

#[tokio::test]
async fn test_line_edit_跨文件多编辑() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "aaa\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "a.txt", "start_line": 1, "new_string": "AAA"},
                {"file_path": "b.txt", "start_line": 1, "new_string": "BBB"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("失败"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "AAA\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "BBB\n"
    );
}

#[tokio::test]
async fn test_line_edit_best_effort部分失败() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "f.txt", "start_line": 1, "new_string": "AAA"},
                {"file_path": "f.txt", "start_line": 99, "new_string": "XXX"}
            ]
        }))
        .await
        .unwrap();
    // 第一个应该成功，第二个应该失败
    assert!(result.contains("失败"), "应包含失败: {result}");
    // 从后往前：99 行先执行（失败），1 行后执行（成功）
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "AAA\nbbb\nccc\n");
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit::tests`
Expected: 所有测试 PASS

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "test: add multi-edit bottom-to-top and cross-file tests for LineEdit"
```

---

### Task 4: 注册 LineEdit 到工具系统

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/mod.rs`
- Modify: `peri-middlewares/src/tools/mod.rs`
- Modify: `peri-middlewares/src/middleware/filesystem.rs`
- Modify: `peri-middlewares/src/tool_search/core_tools.rs`

- [ ] **Step 1: 在 mod.rs 中添加 line_edit 模块**

修改 `peri-middlewares/src/tools/filesystem/mod.rs`，在现有 `pub mod` 块中添加：

```rust
pub mod line_edit;
```

在 `pub use` 块中添加：

```rust
pub use line_edit::LineEditTool;
```

- [ ] **Step 2: 在 tools/mod.rs 中导出 LineEditTool**

修改 `peri-middlewares/src/tools/mod.rs`，在 `pub use filesystem::{...}` 中添加 `LineEditTool`：

```rust
pub use filesystem::{
    EditFileTool, FolderOperationsTool, GlobFilesTool, GrepTool, LineEditTool, ReadFileTool,
    WriteFileTool,
};
```

- [ ] **Step 3: 在 FilesystemMiddleware 中添加 lineEdit 模式**

修改 `peri-middlewares/src/middleware/filesystem.rs`：

1. 添加 import：
```rust
use crate::tools::{LineEditTool, /* ... existing imports ... */};
```

2. 在 `FilesystemMiddleware` 结构体中添加 `line_edit_mode` 字段：
```rust
pub struct FilesystemMiddleware {
    hashline_mode: bool,
    line_edit_mode: bool,
    snapshot_cache: SnapshotCache,
}
```

3. 更新 `new()` 和 builder 方法：
```rust
impl FilesystemMiddleware {
    pub fn new() -> Self {
        Self {
            hashline_mode: false,
            line_edit_mode: false,
            snapshot_cache: new_snapshot_cache(),
        }
    }

    pub fn with_hashline_mode(mut self, enabled: bool) -> Self {
        self.hashline_mode = enabled;
        self
    }

    pub fn with_line_edit_mode(mut self, enabled: bool) -> Self {
        self.line_edit_mode = enabled;
        self
    }

    pub fn with_snapshot_cache(mut self, cache: SnapshotCache) -> Self {
        self.snapshot_cache = cache;
        self
    }
```

4. 更新 `build_tools_with_hashline` 为 `build_tools_with_mode`（或添加新分支），在 edit_tool 选择逻辑中：
```rust
let edit_tool: Box<dyn BaseTool> = if hashline_mode {
    Box::new(HashlineEditTool::new(cwd, cache))
} else if line_edit_mode {
    Box::new(LineEditTool::new(cwd))
} else {
    Box::new(EditFileTool::new(cwd))
};
```

5. 添加 `tool_names_line_edit()` 方法：
```rust
pub fn tool_names_line_edit() -> Vec<&'static str> {
    vec![
        "Read",
        "Write",
        "LineEdit",
        "Glob",
        "Grep",
        "folder_operations",
    ]
}
```

6. 更新 `collect_tools` 传递 `line_edit_mode`。

- [ ] **Step 4: 在 core_tools.rs 中添加常量**

修改 `peri-middlewares/src/tool_search/core_tools.rs`：

1. 添加常量：
```rust
pub const TOOL_LINE_EDIT: &str = "LineEdit";
```

2. 在 `CORE_TOOLS` 中添加 `TOOL_LINE_EDIT`。

3. 更新注释说明。

- [ ] **Step 5: 运行编译和测试**

Run: `cargo build -p peri-middlewares && cargo test -p peri-middlewares --lib`
Expected: 编译成功，所有现有测试 PASS

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/mod.rs peri-middlewares/src/tools/mod.rs peri-middlewares/src/middleware/filesystem.rs peri-middlewares/src/tool_search/core_tools.rs
git commit -m "feat: register LineEdit tool in tool system with lineEdit beta flag"
```

---

### Task 5: Beta 配置和 Builder 集成

**Files:**
- Modify: `peri-acp/src/provider/config.rs`
- Modify: `peri-acp/src/agent/builder.rs`
- Modify: `peri-tui/src/app/betas_panel.rs`

- [ ] **Step 1: 添加 lineEdit 到 BetasConfig**

修改 `peri-acp/src/provider/config.rs` 的 `BetasConfig`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BetasConfig {
    #[serde(default)]
    pub hashline: bool,
    #[serde(default)]
    pub line_edit: bool,
}
```

- [ ] **Step 2: 更新 builder.rs 读取 lineEdit flag**

修改 `peri-acp/src/agent/builder.rs`，在 `hashline_mode` 附近添加：

```rust
let hashline_mode = peri_config.config.betas.hashline;
let line_edit_mode = peri_config.config.betas.line_edit;
```

更新 `FilesystemMiddleware` 构建：

```rust
let filesystem_middleware = FilesystemMiddleware::new()
    .with_hashline_mode(hashline_mode)
    .with_line_edit_mode(line_edit_mode)
    .with_snapshot_cache(snapshot_cache.clone());
```

更新 `build_tools_with_hashline` 调用，传递 `line_edit_mode`：

```rust
FilesystemMiddleware::build_tools_with_mode(&cwd, hashline_mode, line_edit_mode, Some(snapshot_cache));
```

- [ ] **Step 3: 更新 TUI betas_panel.rs**

修改 `peri-tui/src/app/betas_panel.rs`：

1. 更新 `BETA_KEYS`：
```rust
const BETA_KEYS: &[&str] = &["hashline", "lineEdit"];
```

2. 在 `from_config` 的 match 中添加 `lineEdit` 分支：
```rust
"lineEdit" => BetaEntry {
    key: key.to_string(),
    label: "LineEdit".to_string(),
    description: "基于行号的精确编辑模式".to_string(),
    enabled: cfg.config.betas.line_edit,
},
```

3. 在 `apply_to_config` 中添加：
```rust
if entry.key == "lineEdit" {
    cfg.config.betas.line_edit = entry.enabled;
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo build -p peri-acp -p peri-tui`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add peri-acp/src/provider/config.rs peri-acp/src/agent/builder.rs peri-tui/src/app/betas_panel.rs
git commit -m "feat: add lineEdit beta config and integrate with builder"
```

---

### Task 6: 全量测试和集成验证

**Files:**
- 无新文件，运行现有测试验证

- [ ] **Step 1: 运行全量测试**

Run: `cargo test`
Expected: 所有测试 PASS

- [ ] **Step 2: 验证 LineEdit 在 betas 关闭时不生效**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::edit::tests`
Expected: 旧 Edit 测试全部 PASS（lineEdit 关闭时 Edit 工具正常工作）

- [ ] **Step 3: Commit**

```bash
git commit --allow-empty -m "verify: LineEdit integration tests all pass"
```

---

### Task 7: Revert HashlineEdit 并清理

**Files:**
- Delete: `peri-middlewares/src/tools/hashline/` （整个目录）
- Modify: `peri-middlewares/src/tools/filesystem/read.rs` — 移除 hashline 依赖
- Modify: `peri-middlewares/src/tools/mod.rs` — 移除 hashline 导出
- Modify: `peri-middlewares/src/middleware/filesystem.rs` — 移除 hashline 相关逻辑
- Modify: `peri-tui/src/app/betas_panel.rs` — 移除 hashline 条目
- Modify: `peri-acp/src/provider/config.rs` — 移除 hashline 字段
- Modify: `peri-acp/src/agent/builder.rs` — 移除 hashline 逻辑
- Modify: `peri-middlewares/src/tool_search/core_tools.rs` — 移除 TOOL_HASHLINE_EDIT

> **注意**：此 Task 应在 LineEdit 经过实际验证后再执行。LineEdit 稳定前，hashline 保留作为 fallback。

- [ ] **Step 1: 删除 hashline 目录**

```bash
rm -rf peri-middlewares/src/tools/hashline/
```

- [ ] **Step 2: 清理 read.rs — 移除 hashline 依赖**

从 `peri-middlewares/src/tools/filesystem/read.rs` 中：
1. 移除 `use crate::tools::hashline::SnapshotCache;` import
2. 从 `ReadFileTool` 结构体中移除 `snapshot_cache` 和 `hashline_mode` 字段
3. 移除 `with_hashline` 构造方法
4. 移除 `invoke` 中的 `if self.hashline_mode` 分支（整个 block）
5. 保留标准输出路径

- [ ] **Step 3: 清理 tools/mod.rs**

从 `peri-middlewares/src/tools/mod.rs` 中：
1. 移除 `pub mod hashline;`
2. 移除 `pub use hashline::{new_snapshot_cache, HashlineEditTool, SnapshotCache};`

- [ ] **Step 4: 清理 filesystem middleware**

从 `peri-middlewares/src/middleware/filesystem.rs` 中：
1. 移除 `HashlineEditTool`, `new_snapshot_cache`, `SnapshotCache` 的 import
2. 移除 `hashline_mode` 字段
3. 移除 `with_hashline_mode` 方法
4. 移除 `tool_names_hashline()` 方法
5. 移除 edit_tool 选择中的 hashline 分支
6. 移除 `build_tools_with_hashline` 中的 hashline 相关参数（或简化方法签名）

- [ ] **Step 5: 清理 betas_panel.rs**

从 `peri-tui/src/app/betas_panel.rs` 中：
1. 从 `BETA_KEYS` 移除 `"hashline"`
2. 移除 match 中的 `"hashline"` 分支
3. 移除 `apply_to_config` 中的 hashline 分支

- [ ] **Step 6: 清理 config.rs**

从 `peri-acp/src/provider/config.rs` 的 `BetasConfig` 中移除 `hashline` 字段。

- [ ] **Step 7: 清理 builder.rs**

从 `peri-acp/src/agent/builder.rs` 中移除 `hashline_mode` 变量和相关传递。

- [ ] **Step 8: 清理 core_tools.rs**

从 `peri-middlewares/src/tool_search/core_tools.rs` 中：
1. 移除 `TOOL_HASHLINE_EDIT` 常量
2. 从 `CORE_TOOLS` 中移除 `TOOL_HASHLINE_EDIT`
3. 更新注释

- [ ] **Step 9: 编译和全量测试**

Run: `cargo build && cargo test`
Expected: 编译成功，所有测试 PASS，无 hashline 残留引用

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "revert: remove HashlineEdit and hashline beta, replaced by LineEdit"
```

---

### Task 8: 更新 CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: 更新 Beta 功能开关表格**

将 `hashline` 行替换为：

```markdown
| `lineEdit` | 启用行号编辑模式——Edit 替换为 LineEdit（基于行号的精确编辑，支持 start_word/end_word 行内定位、多编辑从后往前应用） |
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with LineEdit beta, remove hashline"
```
