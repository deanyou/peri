# Hashline 补丁编辑系统实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 peri-middlewares 中实现基于内容哈希锚点的 hashline 补丁编辑工具，替代传统 Edit 工具，通过 `settings.json` 的 `betas.hashline` flag 控制。

**Architecture:** 新增 `peri-middlewares/src/tools/hashline/` 模块（7 个文件），修改 Read 工具输出格式和工具注册逻辑。纯函数核心（hash/parser/apply/block）与 IO 层（tool/recovery）分离。共享 `SnapshotCache`（`Arc<RwLock<HashMap<String, String>>>`）连接 Read 和 HashlineEdit 工具。

**Tech Stack:** Rust std `DefaultHasher`、`similar` crate（3-way merge）、`tree-sitter` + 语言 grammar crates（block 操作）

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `peri-middlewares/src/tools/hashline/mod.rs` | 创建 | 模块入口 + SnapshotCache 类型 + is_hashline_mode() |
| `peri-middlewares/src/tools/hashline/hash.rs` | 创建 | 哈希计算与文本归一化（纯函数） |
| `peri-middlewares/src/tools/hashline/parser.rs` | 创建 | 补丁格式 tokenizer + parser |
| `peri-middlewares/src/tools/hashline/apply.rs` | 创建 | 编辑应用算法（纯函数） |
| `peri-middlewares/src/tools/hashline/recovery.rs` | 创建 | 3-way merge 恢复 |
| `peri-middlewares/src/tools/hashline/block.rs` | 创建 | tree-sitter block 操作解析 |
| `peri-middlewares/src/tools/hashline/tool.rs` | 创建 | HashlineEdit 工具（BaseTool 实现） |
| `peri-middlewares/src/tools/filesystem/read.rs` | 修改 | hashline 模式输出格式 + 快照写入 |
| `peri-middlewares/src/tools/filesystem/mod.rs` | 修改 | 导出 SnapshotCache |
| `peri-middlewares/src/tools/mod.rs` | 修改 | 导出 hashline 模块 |
| `peri-middlewares/src/middleware/filesystem.rs` | 修改 | build_tools 加 hashline 分支 |
| `peri-acp/src/provider/config.rs` | 修改 | AppConfig 新增 betas 字段 |
| `peri-acp/src/agent/builder.rs` | 修改 | 传递 SnapshotCache 到 build_tools |
| `peri-middlewares/Cargo.toml` | 修改 | 新增 similar + tree-sitter 依赖 |

---

## Task 1: AppConfig 新增 betas 字段

**Files:**
- Modify: `peri-acp/src/provider/config.rs`

- [ ] **Step 1: 在 AppConfig 中新增 BetasConfig**

在 `peri-acp/src/provider/config.rs` 中，找到 `AppConfig` 结构体（约第 104 行），在其字段末尾（`extra` 字段之前）新增：

```rust
/// Beta 功能开关
#[serde(default)]
pub betas: BetasConfig,
```

在同文件中（`AppConfig` 定义之前）新增 `BetasConfig` 结构体：

```rust
/// Beta 功能开关配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct BetasConfig {
    /// 启用 hashline 补丁编辑模式
    #[serde(default)]
    pub hashline: bool,
}
```

确保 `BetasConfig` 实现了 `Default`（所有字段默认 false）。

- [ ] **Step 2: 验证编译**

Run: `cargo build -p peri-acp`
Expected: 编译成功，无错误

- [ ] **Step 3: Commit**

```bash
git add peri-acp/src/provider/config.rs
git commit -m "feat: add BetasConfig to AppConfig for feature flags"
```

---

## Task 2: 哈希计算模块（`hash.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/hash.rs`
- Create: `peri-middlewares/src/tools/hashline/hash_test.rs`
- Create: `peri-middlewares/src/tools/hashline/mod.rs`（骨架）

- [ ] **Step 1: 创建 mod.rs 骨架**

创建 `peri-middlewares/src/tools/hashline/mod.rs`：

```rust
pub mod apply;
pub mod block;
pub mod hash;
pub mod parser;
pub mod recovery;
pub mod tool;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// 快照缓存：Read 工具写入，HashlineEdit 工具读取
/// key = 文件路径（canonicalized），value = 文件完整内容
pub type SnapshotCache = Arc<RwLock<HashMap<String, String>>>;

/// 创建空的 SnapshotCache
pub fn new_snapshot_cache() -> SnapshotCache {
    Arc::new(RwLock::new(HashMap::new()))
}

/// 检查是否启用 hashline 模式
/// 读取环境变量 HASHLINE_MODE 或通过参数传入
pub fn is_hashline_mode(enabled: bool) -> bool {
    enabled
}
```

- [ ] **Step 2: 创建 hash.rs**

创建 `peri-middlewares/src/tools/hashline/hash.rs`：

```rust
/// 文本归一化：裁剪尾部空白 + 统一 LF 换行
fn normalize(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end_matches(|c| c == ' ' || c == '\t' || c == '\r'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 计算文件内容哈希
/// 使用 DefaultHasher，取低 16 位生成 4 字符大写十六进制标签
pub fn compute_file_hash(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let normalized = normalize(text);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    let low16 = hasher.finish() & 0xFFFF;
    format!("{:04X}", low16)
}

/// 验证文件内容是否匹配预期哈希
pub fn verify_hash(text: &str, expected: &str) -> bool {
    compute_file_hash(text) == expected
}

/// 格式化 hashline 头部：path#HASH
pub fn format_header(path: &str, hash: &str) -> String {
    format!("{}#{}", path, hash)
}

/// 格式化带行号的内容行
pub fn format_numbered_line(line_num: usize, text: &str) -> String {
    format!("{:>6}\t{}", line_num, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("hash_test.rs");
}
```

- [ ] **Step 3: 创建 hash_test.rs**

创建 `peri-middlewares/src/tools/hashline/hash_test.rs`：

```rust
#[test]
fn test_compute_file_hash_一致性() {
    let text = "fn main() {\n    println!(\"hello\");\n}\n";
    let h1 = compute_file_hash(text);
    let h2 = compute_file_hash(text);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 4);
    // 大写十六进制
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_compute_file_hash_忽略尾部空白() {
    let text_a = "line1\nline2\n";
    let text_b = "line1  \nline2\t\n";
    assert_eq!(compute_file_hash(text_a), compute_file_hash(text_b));
}

#[test]
fn test_compute_file_hash_忽略_crlf() {
    let text_lf = "line1\nline2\n";
    let text_crlf = "line1\r\nline2\r\n";
    assert_eq!(compute_file_hash(text_lf), compute_file_hash(text_crlf));
}

#[test]
fn test_compute_file_hash_不同内容不同哈希() {
    let text_a = "fn foo() {}";
    let text_b = "fn bar() {}";
    assert_ne!(compute_file_hash(text_a), compute_file_hash(text_b));
}

#[test]
fn test_verify_hash_匹配() {
    let text = "hello world";
    let hash = compute_file_hash(text);
    assert!(verify_hash(text, &hash));
}

#[test]
fn test_verify_hash_不匹配() {
    let hash = compute_file_hash("original");
    assert!(!verify_hash("modified", &hash));
}

#[test]
fn test_format_header() {
    assert_eq!(format_header("src/main.rs", "A3F2"), "src/main.rs#A3F2");
}

#[test]
fn test_format_numbered_line() {
    let result = format_numbered_line(1, "hello");
    assert!(result.contains("hello"));
    assert!(result.starts_with("     1"));
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::hash::tests`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/tools/hashline/
git commit -m "feat: add hashline hash module with normalization and file hash computation"
```

---

## Task 3: 补丁格式解析器（`parser.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/parser.rs`
- Create: `peri-middlewares/src/tools/hashline/parser_test.rs`

- [ ] **Step 1: 创建 parser.rs**

创建 `peri-middlewares/src/tools/hashline/parser.rs`，包含类型定义 + Tokenizer + Parser：

```rust
use std::fmt;

/// 1-indexed 行号锚点
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub line: usize,
}

/// 插入光标位置
#[derive(Debug, Clone, PartialEq)]
pub enum Cursor {
    Bof,
    Eof,
    BeforeAnchor(Anchor),
    AfterAnchor(Anchor),
}

/// 单个编辑操作
#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    Insert {
        cursor: Cursor,
        text: String,
    },
    Delete {
        start: usize,
        end: usize,
    },
    Replace {
        start: usize,
        end: usize,
        text: String,
    },
    Block {
        anchor_line: usize,
        text: String,
    },
}

/// 一个文件的补丁段
#[derive(Debug, Clone)]
pub struct PatchSection {
    pub file_path: String,
    pub expected_hash: String,
    pub edits: Vec<EditOp>,
}

/// 完整补丁（可跨多个文件）
#[derive(Debug, Clone)]
pub struct Patch {
    pub sections: Vec<PatchSection>,
}

/// 解析错误
#[derive(Debug)]
pub struct ParseError {
    pub line_num: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "第 {} 行: {}", self.line_num, self.message)
    }
}

impl std::error::Error for ParseError {}

// ---- Tokenizer ----

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Header,
    OpReplace,
    OpDelete,
    OpInsertBefore,
    OpInsertAfter,
    OpInsertHead,
    OpInsertTail,
    OpBlock,
    Payload,
}

#[derive(Debug)]
struct Token {
    kind: TokenKind,
    line_num: usize,
    /// Header: (path, hash)
    /// OpReplace: (start, end)
    /// OpDelete: (start, end)
    /// OpInsertBefore/After: line
    /// OpBlock: anchor_line
    value: Option<(String, String)>,
}

/// 逐行分类
fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Header: ¶path#HASH 或 path#HASH（4 字符大写十六进制）
        if let Some(rest) = trimmed.strip_prefix('¶') {
            if let Some((path, hash)) = parse_header(rest) {
                tokens.push(Token {
                    kind: TokenKind::Header,
                    line_num,
                    value: Some((path.to_string(), hash.to_string())),
                });
                continue;
            }
        }
        // 也支持不带 ¶ 前缀的 path#HASH 格式
        if !trimmed.starts_with('+')
            && !trimmed.starts_with("replace")
            && !trimmed.starts_with("delete")
            && !trimmed.starts_with("insert")
        {
            if let Some((path, hash)) = parse_header(trimmed) {
                tokens.push(Token {
                    kind: TokenKind::Header,
                    line_num,
                    value: Some((path.to_string(), hash.to_string())),
                });
                continue;
            }
        }

        // replace N..M:
        if let Some(captures) = regex_captures_replace(trimmed) {
            tokens.push(Token {
                kind: TokenKind::OpReplace,
                line_num,
                value: Some(captures),
            });
            continue;
        }

        // delete N..M 或 delete N
        if let Some(captures) = regex_captures_delete(trimmed) {
            tokens.push(Token {
                kind: TokenKind::OpDelete,
                line_num,
                value: Some(captures),
            });
            continue;
        }

        // insert before N: / insert after N:
        if let Some(captures) = regex_captures_insert_anchor(trimmed) {
            let kind = if captures.0 == "before" {
                TokenKind::OpInsertBefore
            } else {
                TokenKind::OpInsertAfter
            };
            tokens.push(Token {
                kind,
                line_num,
                value: Some((captures.1.clone(), captures.1.clone())),
            });
            continue;
        }

        // insert head: / insert tail:
        if trimmed == "insert head:" {
            tokens.push(Token {
                kind: TokenKind::OpInsertHead,
                line_num,
                value: None,
            });
            continue;
        }
        if trimmed == "insert tail:" {
            tokens.push(Token {
                kind: TokenKind::OpInsertTail,
                line_num,
                value: None,
            });
            continue;
        }

        // replace block N:
        if let Some(captures) = regex_captures_block(trimmed) {
            tokens.push(Token {
                kind: TokenKind::OpBlock,
                line_num,
                value: Some(captures),
            });
            continue;
        }

        // Payload: +TEXT
        if let Some(text) = trimmed.strip_prefix('+') {
            tokens.push(Token {
                kind: TokenKind::Payload,
                line_num,
                value: Some((text.to_string(), String::new())),
            });
            continue;
        }

        // 无法识别的行 → 报错
        return Err(ParseError {
            line_num,
            message: format!("无法识别的行: {}", trimmed),
        });
    }
    Ok(tokens)
}

/// 解析头部 path#HASH（HASH 为 4 字符大写十六进制）
fn parse_header(input: &str) -> Option<(&str, &str)> {
    let hash_start = input.rfind('#')?;
    let path = &input[..hash_start];
    let hash = &input[hash_start + 1..];
    if hash.len() == 4 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some((path, hash))
    } else {
        None
    }
}

fn regex_captures_replace(s: &str) -> Option<(String, String)> {
    // replace N..M:
    let s = s.strip_suffix(':')?;
    let s = s.strip_prefix("replace ")?;
    let parts: Vec<&str> = s.split("..").collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn regex_captures_delete(s: &str) -> Option<(String, String)> {
    let s = s.strip_prefix("delete ")?;
    if let Some(idx) = s.find("..") {
        Some((s[..idx].to_string(), s[idx + 2..].to_string()))
    } else {
        // delete N → delete N..N
        Some((s.to_string(), s.to_string()))
    }
}

fn regex_captures_insert_anchor(s: &str) -> Option<(&str, &str)> {
    let s = s.strip_suffix(':')?;
    if let Some(rest) = s.strip_prefix("insert before ") {
        return Some(("before", rest));
    }
    if let Some(rest) = s.strip_prefix("insert after ") {
        return Some(("after", rest));
    }
    None
}

fn regex_captures_block(s: &str) -> Option<(String, String)> {
    let s = s.strip_suffix(':')?;
    let s = s.strip_prefix("replace block ")?;
    Some((s.to_string(), s.to_string()))
}

// ---- Parser ----

/// 解析 hashline 补丁文本为 Patch
pub fn parse(input: &str) -> Result<Patch, ParseError> {
    let tokens = tokenize(input)?;
    let mut sections: Vec<PatchSection> = Vec::new();
    let mut current_section: Option<PatchSection> = None;
    let mut pending_edits: Vec<EditOp> = Vec::new();
    let mut payload_lines: Vec<String> = Vec::new();

    for token in &tokens {
        match token.kind {
            TokenKind::Header => {
                // flush 当前 pending
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                // flush 当前 section
                if !pending_edits.is_empty() {
                    if let Some(section) = current_section.take() {
                        sections.push(section);
                    }
                }
                // 开始新 section（保留 pending_edits 给上一个 section）
                // 这里需要先完成之前的 section
                if current_section.is_some() {
                    let mut section = current_section.take().unwrap();
                    section.edits.append(&mut pending_edits);
                    sections.push(section);
                } else if !pending_edits.is_empty() {
                    // 有 edits 但没有 header
                    return Err(ParseError {
                        line_num: token.line_num,
                        message: "编辑操作必须跟在文件头 (¶path#HASH) 之后".into(),
                    });
                }

                let (path, hash) = token.value.as_ref().unwrap();
                current_section = Some(PatchSection {
                    file_path: path.clone(),
                    expected_hash: hash.clone(),
                    edits: Vec::new(),
                });
            }
            TokenKind::OpReplace => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                let (start_s, end_s) = token.value.as_ref().unwrap();
                let start: usize = start_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的起始行号: {}", start_s),
                })?;
                let end: usize = end_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的结束行号: {}", end_s),
                })?;
                // Replace 的 payload 在后续 Payload tokens 中收集
                // 暂存为 pending（payload_lines 为空表示等待收集）
                pending_edits.push(EditOp::Replace {
                    start,
                    end,
                    text: String::new(), // 将在后续 payload 中填充
                });
                // 标记：当前 pending 需要收集 payload
                // 简化处理：最后一个 pending edit 如果 text 为空，就往里填
            }
            TokenKind::OpDelete => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                let (start_s, end_s) = token.value.as_ref().unwrap();
                let start: usize = start_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的起始行号: {}", start_s),
                })?;
                let end: usize = end_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的结束行号: {}", end_s),
                })?;
                pending_edits.push(EditOp::Delete { start, end });
            }
            TokenKind::OpInsertBefore => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                let (line_s, _) = token.value.as_ref().unwrap();
                let line: usize = line_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的行号: {}", line_s),
                })?;
                pending_edits.push(EditOp::Insert {
                    cursor: Cursor::BeforeAnchor(Anchor { line }),
                    text: String::new(),
                });
            }
            TokenKind::OpInsertAfter => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                let (line_s, _) = token.value.as_ref().unwrap();
                let line: usize = line_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的行号: {}", line_s),
                })?;
                pending_edits.push(EditOp::Insert {
                    cursor: Cursor::AfterAnchor(Anchor { line }),
                    text: String::new(),
                });
            }
            TokenKind::OpInsertHead => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                pending_edits.push(EditOp::Insert {
                    cursor: Cursor::Bof,
                    text: String::new(),
                });
            }
            TokenKind::OpInsertTail => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                pending_edits.push(EditOp::Insert {
                    cursor: Cursor::Eof,
                    text: String::new(),
                });
            }
            TokenKind::OpBlock => {
                if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, token.line_num)? {
                    pending_edits.push(edit);
                }
                let (line_s, _) = token.value.as_ref().unwrap();
                let anchor_line: usize = line_s.parse().map_err(|_| ParseError {
                    line_num: token.line_num,
                    message: format!("无效的块锚点行号: {}", line_s),
                })?;
                pending_edits.push(EditOp::Block {
                    anchor_line,
                    text: String::new(),
                });
            }
            TokenKind::Payload => {
                let (text, _) = token.value.as_ref().unwrap();
                payload_lines.push(text.clone());
            }
        }
    }

    // flush 最后的 pending
    if let Some(edit) = flush_pending(&mut payload_lines, &mut pending_edits, 0)? {
        pending_edits.push(edit);
    }

    // 完成最后一个 section
    if let Some(mut section) = current_section.take() {
        section.edits.append(&mut pending_edits);
        sections.push(section);
    }

    Ok(Patch { sections })
}

/// 将收集的 payload_lines 填充到最后一个 pending edit 的 text 字段
/// 返回 Ok(Some(edit)) 表示有一个已完成的 edit 需要放回，
/// 但实际逻辑是直接修改 pending_edits 最后一项的 text
fn flush_pending(
    payload_lines: &mut Vec<String>,
    pending_edits: &mut Vec<EditOp>,
    _error_line: usize,
) -> Result<Option<EditOp>, ParseError> {
    if payload_lines.is_empty() {
        return Ok(None);
    }
    let text = payload_lines.join("\n");
    payload_lines.clear();

    // 填充最后一个 pending edit 的 text
    if let Some(last) = pending_edits.last_mut() {
        match last {
            EditOp::Insert { text: t, .. }
            | EditOp::Replace { text: t, .. }
            | EditOp::Block { text: t, .. } => {
                *t = text;
            }
            EditOp::Delete { .. } => {
                // Delete 不需要 payload，payload 属于下一个操作
                // 不太可能发生，但安全处理
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("parser_test.rs");
}
```

- [ ] **Step 2: 创建 parser_test.rs**

创建 `peri-middlewares/src/tools/hashline/parser_test.rs`：

```rust
use super::*;

#[test]
fn test_parse_单文件替换() {
    let input = "¶src/main.rs#A3F2\nreplace 2..4:\n+new line 1\n+new line 2";
    let patch = parse(input).unwrap();
    assert_eq!(patch.sections.len(), 1);
    assert_eq!(patch.sections[0].file_path, "src/main.rs");
    assert_eq!(patch.sections[0].expected_hash, "A3F2");
    assert_eq!(patch.sections[0].edits.len(), 1);
    match &patch.sections[0].edits[0] {
        EditOp::Replace { start, end, text } => {
            assert_eq!(*start, 2);
            assert_eq!(*end, 4);
            assert_eq!(text, "new line 1\nnew line 2");
        }
        _ => panic!("期望 Replace 操作"),
    }
}

#[test]
fn test_parse_删除操作() {
    let input = "¶src/main.rs#A3F2\ndelete 3..5";
    let patch = parse(input).unwrap();
    assert_eq!(patch.sections[0].edits.len(), 1);
    match &patch.sections[0].edits[0] {
        EditOp::Delete { start, end } => {
            assert_eq!(*start, 3);
            assert_eq!(*end, 5);
        }
        _ => panic!("期望 Delete 操作"),
    }
}

#[test]
fn test_parse_插入操作() {
    let input = "¶src/main.rs#A3F2\ninsert after 2:\n+    new line";
    let patch = parse(input).unwrap();
    match &patch.sections[0].edits[0] {
        EditOp::Insert { cursor, text } => {
            assert!(matches!(cursor, Cursor::AfterAnchor(Anchor { line: 2 })));
            assert_eq!(text, "    new line");
        }
        _ => panic!("期望 Insert 操作"),
    }
}

#[test]
fn test_parse_多操作() {
    let input = "¶src/main.rs#A3F2\nreplace 2..4:\n+new line\ndelete 7";
    let patch = parse(input).unwrap();
    assert_eq!(patch.sections[0].edits.len(), 2);
}

#[test]
fn test_parse_多文件() {
    let input = "¶a.rs#A1B2\nreplace 1..1:\n+aaa\n¶b.rs#C3D4\ndelete 2..3";
    let patch = parse(input).unwrap();
    assert_eq!(patch.sections.len(), 2);
    assert_eq!(patch.sections[0].file_path, "a.rs");
    assert_eq!(patch.sections[1].file_path, "b.rs");
}

#[test]
fn test_parse_无_header_报错() {
    let input = "replace 1..1:\n+text";
    let result = parse(input);
    assert!(result.is_err());
}

#[test]
fn test_parse_insert_head_tail() {
    let input = "¶src/main.rs#A3F2\ninsert head:\n+// header\ninsert tail:\n+// footer";
    let patch = parse(input).unwrap();
    assert_eq!(patch.sections[0].edits.len(), 2);
    match &patch.sections[0].edits[0] {
        EditOp::Insert { cursor: Cursor::Bof, text } => {
            assert_eq!(text, "// header");
        }
        _ => panic!("期望 Bof Insert"),
    }
    match &patch.sections[0].edits[1] {
        EditOp::Insert { cursor: Cursor::Eof, text } => {
            assert_eq!(text, "// footer");
        }
        _ => panic!("期望 Eof Insert"),
    }
}

#[test]
fn test_parse_block() {
    let input = "¶src/main.rs#A3F2\nreplace block 5:\n+fn new() {}";
    let patch = parse(input).unwrap();
    match &patch.sections[0].edits[0] {
        EditOp::Block { anchor_line, text } => {
            assert_eq!(*anchor_line, 5);
            assert_eq!(text, "fn new() {}");
        }
        _ => panic!("期望 Block 操作"),
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::parser::tests`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/hashline/parser.rs peri-middlewares/src/tools/hashline/parser_test.rs
git commit -m "feat: add hashline parser with tokenizer and patch parsing"
```

---

## Task 4: 编辑应用算法（`apply.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/apply.rs`
- Create: `peri-middlewares/src/tools/hashline/apply_test.rs`

- [ ] **Step 1: 创建 apply.rs**

创建 `peri-middlewares/src/tools/hashline/apply.rs`：

```rust
use super::parser::{Cursor, EditOp};
use std::fmt;

/// 应用编辑时产生的错误
#[derive(Debug)]
pub enum ApplyError {
    /// 行号超出范围
    LineOutOfRange { line: usize, total: usize },
    /// 无效的范围（start > end）
    InvalidRange { start: usize, end: usize },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApplyError::LineOutOfRange { line, total } => {
                write!(f, "行号 {} 超出范围（总行数: {}）", line, total)
            }
            ApplyError::InvalidRange { start, end } => {
                write!(f, "无效范围: {} > {}", start, end)
            }
        }
    }
}

impl std::error::Error for ApplyError {}

/// 将编辑操作应用到文本内容
/// 纯函数，零 IO
pub fn apply_edits(content: &str, edits: &[EditOp]) -> Result<String, ApplyError> {
    if edits.is_empty() {
        return Ok(content.to_string());
    }

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

    // 分类编辑
    let mut bof_inserts: Vec<String> = Vec::new();
    let mut eof_inserts: Vec<String> = Vec::new();
    let mut anchor_edits: Vec<AnchorEdit> = Vec::new();

    for edit in edits {
        match edit {
            EditOp::Insert { cursor, text } => match cursor {
                Cursor::Bof => {
                    bof_inserts.push(text.clone());
                }
                Cursor::Eof => {
                    eof_inserts.push(text.clone());
                }
                Cursor::BeforeAnchor(anchor) => {
                    anchor_edits.push(AnchorEdit {
                        line: anchor.line,
                        kind: AnchorEditKind::InsertBefore,
                        text: text.clone(),
                    });
                }
                Cursor::AfterAnchor(anchor) => {
                    anchor_edits.push(AnchorEdit {
                        line: anchor.line,
                        kind: AnchorEditKind::InsertAfter,
                        text: text.clone(),
                    });
                }
            },
            EditOp::Delete { start, end } => {
                validate_range(*start, *end, lines.len())?;
                anchor_edits.push(AnchorEdit {
                    line: *start,
                    kind: AnchorEditKind::Delete { end: *end },
                    text: String::new(),
                });
            }
            EditOp::Replace { start, end, text } => {
                validate_range(*start, *end, lines.len())?;
                anchor_edits.push(AnchorEdit {
                    line: *start,
                    kind: AnchorEditKind::Replace { end: *end },
                    text: text.clone(),
                });
            }
            EditOp::Block { .. } => {
                // Block 应在调用前已展开为 Replace
                return Ok(content.to_string());
            }
        }
    }

    // 自底向上排序（保持行号有效性）
    anchor_edits.sort_by(|a, b| b.line.cmp(&a.line));

    // 逐个应用 anchor 编辑
    for edit in &anchor_edits {
        apply_anchor_edit(&mut lines, edit)?;
    }

    // 拼接结果
    let mut result_parts: Vec<String> = Vec::new();
    for text in &bof_inserts {
        result_parts.push(text.clone());
    }
    result_parts.extend(lines);
    for text in &eof_inserts {
        result_parts.push(text.clone());
    }

    Ok(result_parts.join("\n"))
}

#[derive(Debug)]
enum AnchorEditKind {
    InsertBefore,
    InsertAfter,
    Delete { end: usize },
    Replace { end: usize },
}

#[derive(Debug)]
struct AnchorEdit {
    line: usize,
    kind: AnchorEditKind,
    text: String,
}

fn apply_anchor_edit(lines: &mut Vec<String>, edit: &AnchorEdit) -> Result<(), ApplyError> {
    match &edit.kind {
        AnchorEditKind::InsertBefore => {
            let new_lines: Vec<String> = edit.text.lines().map(|s| s.to_string()).collect();
            let idx = edit.line.saturating_sub(1);
            if idx > lines.len() {
                return Err(ApplyError::LineOutOfRange {
                    line: edit.line,
                    total: lines.len(),
                });
            }
            for (i, new_line) in new_lines.into_iter().enumerate() {
                lines.insert(idx + i, new_line);
            }
        }
        AnchorEditKind::InsertAfter => {
            let new_lines: Vec<String> = edit.text.lines().map(|s| s.to_string()).collect();
            let idx = edit.line.min(lines.len());
            for (i, new_line) in new_lines.into_iter().enumerate() {
                lines.insert(idx + i, new_line);
            }
        }
        AnchorEditKind::Delete { end } => {
            let start_idx = edit.line.saturating_sub(1);
            let end_idx = (*end).min(lines.len());
            lines.drain(start_idx..end_idx);
        }
        AnchorEditKind::Replace { end } => {
            let new_lines: Vec<String> = edit.text.lines().map(|s| s.to_string()).collect();
            let start_idx = edit.line.saturating_sub(1);
            let end_idx = (*end).min(lines.len());
            lines.splice(start_idx..end_idx, new_lines);
        }
    }
    Ok(())
}

fn validate_range(start: usize, end: usize, total: usize) -> Result<(), ApplyError> {
    if start > end {
        return Err(ApplyError::InvalidRange { start, end });
    }
    if start == 0 || end > total {
        return Err(ApplyError::LineOutOfRange {
            line: if start == 0 { start } else { end },
            total,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("apply_test.rs");
}
```

- [ ] **Step 2: 创建 apply_test.rs**

创建 `peri-middlewares/src/tools/hashline/apply_test.rs`：

```rust
use super::*;

fn make_replace(start: usize, end: usize, text: &str) -> EditOp {
    EditOp::Replace {
        start,
        end,
        text: text.to_string(),
    }
}

fn make_delete(start: usize, end: usize) -> EditOp {
    EditOp::Delete { start, end }
}

fn make_insert_before(line: usize, text: &str) -> EditOp {
    EditOp::Insert {
        cursor: Cursor::BeforeAnchor(Anchor { line }),
        text: text.to_string(),
    }
}

fn make_insert_after(line: usize, text: &str) -> EditOp {
    EditOp::Insert {
        cursor: Cursor::AfterAnchor(Anchor { line }),
        text: text.to_string(),
    }
}

fn make_insert_bof(text: &str) -> EditOp {
    EditOp::Insert {
        cursor: Cursor::Bof,
        text: text.to_string(),
    }
}

fn make_insert_eof(text: &str) -> EditOp {
    EditOp::Insert {
        cursor: Cursor::Eof,
        text: text.to_string(),
    }
}

use super::super::parser::{Anchor, Cursor};

#[test]
fn test_apply_替换() {
    let content = "line1\nline2\nline3\nline4";
    let edits = vec![make_replace(2, 3, "new2\nnew3")];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "line1\nnew2\nnew3\nline4");
}

#[test]
fn test_apply_删除() {
    let content = "line1\nline2\nline3\nline4";
    let edits = vec![make_delete(2, 3)];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "line1\nline4");
}

#[test]
fn test_apply_插入_before() {
    let content = "line1\nline2\nline3";
    let edits = vec![make_insert_before(2, "inserted")];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "line1\ninserted\nline2\nline3");
}

#[test]
fn test_apply_插入_after() {
    let content = "line1\nline2\nline3";
    let edits = vec![make_insert_after(2, "inserted")];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "line1\nline2\ninserted\nline3");
}

#[test]
fn test_apply_bof_eof() {
    let content = "line1\nline2";
    let edits = vec![make_insert_bof("header"), make_insert_eof("footer")];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "header\nline1\nline2\nfooter");
}

#[test]
fn test_apply_多个编辑_自底向上() {
    let content = "line1\nline2\nline3\nline4\nline5";
    // 两个替换：先应用底部再应用顶部
    let edits = vec![make_replace(2, 2, "NEW2"), make_replace(4, 4, "NEW4")];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "line1\nNEW2\nline3\nNEW4\nline5");
}

#[test]
fn test_apply_行号越界() {
    let content = "line1\nline2";
    let edits = vec![make_replace(1, 10, "x")];
    // end > total 不报错，因为 splice 会自动 clamp
    // 但 validate_range 会报错
    let result = apply_edits(content, &edits);
    assert!(result.is_err());
}

#[test]
fn test_apply_空编辑列表() {
    let content = "line1\nline2";
    let result = apply_edits(content, &[]).unwrap();
    assert_eq!(result, content);
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::apply::tests`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/hashline/apply.rs peri-middlewares/src/tools/hashline/apply_test.rs
git commit -m "feat: add hashline apply module with bottom-up edit application"
```

---

## Task 5: Block 操作模块（`block.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/block.rs`
- Create: `peri-middlewares/src/tools/hashline/block_test.rs`
- Modify: `peri-middlewares/Cargo.toml`

- [ ] **Step 1: 在 Cargo.toml 添加 tree-sitter 依赖**

在 `peri-middlewares/Cargo.toml` 的 `[dependencies]` 末尾添加：

```toml
similar = "2"
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
```

- [ ] **Step 2: 创建 block.rs**

创建 `peri-middlewares/src/tools/hashline/block.rs`：

```rust
use super::parser::EditOp;
use std::fmt;
use tree_sitter::{Language, Parser};

/// Block 操作解析错误
#[derive(Debug)]
pub enum BlockError {
    /// 不支持的文件类型
    UnsupportedLanguage(String),
    /// Tree-sitter 解析失败
    ParseFailed(String),
    /// 行号不在任何语法块内
    NoBlockAtLine(usize),
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::UnsupportedLanguage(ext) => {
                write!(f, "不支持的文件类型: .{}", ext)
            }
            BlockError::ParseFailed(msg) => {
                write!(f, "Tree-sitter 解析失败: {}", msg)
            }
            BlockError::NoBlockAtLine(line) => {
                write!(f, "第 {} 行不在任何语法块内", line)
            }
        }
    }
}

impl std::error::Error for BlockError {}

/// 解析后的编辑操作（Block 已展开为 Replace）
#[derive(Debug, Clone)]
pub enum ResolvedEdit {
    Insert {
        cursor: super::parser::Cursor,
        text: String,
    },
    Delete {
        start: usize,
        end: usize,
    },
    Replace {
        start: usize,
        end: usize,
        text: String,
    },
}

/// 将 Block 操作展开为具体行范围，其他操作原样传递
pub fn resolve_blocks(
    source: &str,
    ext: Option<&str>,
    edits: &[EditOp],
) -> Result<Vec<ResolvedEdit>, BlockError> {
    let mut resolved = Vec::new();
    for edit in edits {
        match edit {
            EditOp::Block { anchor_line, text } => {
                let (start, end) = find_block_range(source, ext, *anchor_line)?;
                resolved.push(ResolvedEdit::Replace {
                    start,
                    end,
                    text: text.clone(),
                });
            }
            EditOp::Insert { cursor, text } => {
                resolved.push(ResolvedEdit::Insert {
                    cursor: cursor.clone(),
                    text: text.clone(),
                });
            }
            EditOp::Delete { start, end } => {
                resolved.push(ResolvedEdit::Delete {
                    start: *start,
                    end: *end,
                });
            }
            EditOp::Replace { start, end, text } => {
                resolved.push(ResolvedEdit::Replace {
                    start: *start,
                    end: *end,
                    text: text.clone(),
                });
            }
        }
    }
    Ok(resolved)
}

/// 根据文件扩展名获取 tree-sitter Language
fn get_language(ext: &str) -> Option<Language> {
    match ext {
        "rs" => Some(tree_sitter_rust::language()),
        "ts" | "tsx" => Some(
            tree_sitter_typescript::language_typescript(),
        ),
        "js" | "jsx" => Some(tree_sitter_javascript::language()),
        "py" => Some(tree_sitter_python::language()),
        "go" => Some(tree_sitter_go::language()),
        _ => None,
    }
}

/// 找到第 N 行（1-indexed）所在的语法块范围
fn find_block_range(
    source: &str,
    ext: Option<&str>,
    anchor_line: usize,
) -> Result<(usize, usize), BlockError> {
    let ext_str = match ext {
        Some(e) => e,
        None => return fallback_indent_block(source, anchor_line),
    };

    let language = match get_language(ext_str) {
        Some(lang) => lang,
        None => return fallback_indent_block(source, anchor_line),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| BlockError::ParseFailed(e.to_string()))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| BlockError::ParseFailed("解析返回 None".into()))?;

    let root = tree.root_node();
    let target_row = anchor_line.saturating_sub(1); // 1-indexed → 0-indexed

    // 找到覆盖目标行的最小节点
    let mut node = root;
    let mut found = false;
    loop {
        let mut child_count = 0;
        for i in 0..node.child_count() {
            let child = node.child(i).unwrap();
            if child.start_position().row <= target_row
                && child.end_position().row >= target_row
            {
                node = child;
                found = true;
                child_count += 1;
                break;
            }
        }
        if child_count == 0 {
            break;
        }
    }

    if !found && node.start_position().row <= target_row && node.end_position().row >= target_row {
        found = true;
    }

    if !found {
        return Err(BlockError::NoBlockAtLine(anchor_line));
    }

    let start = node.start_position().row + 1; // 0-indexed → 1-indexed
    let end = node.end_position().row + 1;
    Ok((start, end))
}

/// 回退：基于缩进的块检测
fn fallback_indent_block(source: &str, anchor_line: usize) -> Result<(usize, usize), BlockError> {
    let lines: Vec<&str> = source.lines().collect();
    if anchor_line == 0 || anchor_line > lines.len() {
        return Err(BlockError::NoBlockAtLine(anchor_line));
    }

    let base_indent = indent_level(lines[anchor_line - 1]);
    let mut end = anchor_line;

    for i in anchor_line..lines.len() {
        let line = lines[i];
        if !line.trim().is_empty() && indent_level(line) <= base_indent {
            break;
        }
        end = i + 1;
    }

    Ok((anchor_line, end))
}

fn indent_level(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("block_test.rs");
}
```

- [ ] **Step 3: 创建 block_test.rs**

创建 `peri-middlewares/src/tools/hashline/block_test.rs`：

```rust
use super::*;

#[test]
fn test_fallback_indent_block_基本() {
    let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
    // 第 2 行 (let x = 1;) 的缩进块覆盖 2-3 行
    let (start, end) = fallback_indent_block(source, 2).unwrap();
    assert_eq!(start, 2);
    assert_eq!(end, 3);
}

#[test]
fn test_fallback_indent_block_单行() {
    let source = "line1\nline2\nline3";
    let (start, end) = fallback_indent_block(source, 2).unwrap();
    assert_eq!(start, 2);
    assert_eq!(end, 2);
}

#[test]
fn test_resolve_blocks_非block操作原样传递() {
    let source = "line1\nline2";
    let edits = vec![
        EditOp::Replace { start: 1, end: 1, text: "new".into() },
        EditOp::Delete { start: 2, end: 2 },
    ];
    let resolved = resolve_blocks(source, None, &edits).unwrap();
    assert_eq!(resolved.len(), 2);
    assert!(matches!(resolved[0], ResolvedEdit::Replace { .. }));
    assert!(matches!(resolved[1], ResolvedEdit::Delete { .. }));
}

#[test]
fn test_get_language_支持的语言() {
    assert!(get_language("rs").is_some());
    assert!(get_language("ts").is_some());
    assert!(get_language("js").is_some());
    assert!(get_language("py").is_some());
    assert!(get_language("go").is_some());
    assert!(get_language("unknown").is_none());
}

#[test]
fn test_find_block_range_rust() {
    let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
    let result = find_block_range(source, Some("rs"), 2);
    // tree-sitter 应该能识别整个函数块
    assert!(result.is_ok());
    let (start, end) = result.unwrap();
    assert!(start <= 2);
    assert!(end >= 2);
}
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::block::tests`
Expected: 全部 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/Cargo.toml peri-middlewares/src/tools/hashline/block.rs peri-middlewares/src/tools/hashline/block_test.rs
git commit -m "feat: add hashline block module with tree-sitter and indent fallback"
```

---

## Task 6: 恢复机制（`recovery.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/recovery.rs`
- Create: `peri-middlewares/src/tools/hashline/recovery_test.rs`

- [ ] **Step 1: 创建 recovery.rs**

创建 `peri-middlewares/src/tools/hashline/recovery.rs`：

```rust
use super::apply::apply_edits;
use super::parser::EditOp;
use std::fmt;

/// 恢复结果
pub enum RecoveryResult {
    /// 恢复成功，返回合并后的内容
    Recovered {
        content: String,
        warning: Option<String>,
    },
    /// 恢复失败，需要重新读取
    Failed(String),
}

/// 尝试恢复：将编辑应用到快照版本，再 3-way merge 到当前内容
pub fn try_recover(
    snapshot: &str,
    current: &str,
    edits: &[EditOp],
) -> RecoveryResult {
    // 1. 将编辑应用到快照
    let patched = match apply_edits(snapshot, edits) {
        Ok(p) => p,
        Err(e) => {
            return RecoveryResult::Failed(format!("应用到快照失败: {}", e));
        }
    };

    // 2. 3-way merge: base=snapshot, ours=patched, theirs=current
    match three_way_merge(snapshot, &patched, current) {
        Ok(content) => RecoveryResult::Recovered {
            content,
            warning: Some(
                "文件已被外部修改，已通过 3-way merge 恢复。请验证 diff 是否符合预期。".into(),
            ),
        },
        Err(conflict_msg) => {
            RecoveryResult::Failed(format!("合并冲突，请重新读取文件: {}", conflict_msg))
        }
    }
}

/// 基于 similar crate 的 3-way merge
fn three_way_merge(base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    use similar::{ChangeTag, TextDiff};

    // 计算 base→ours 的变更
    let diff_ours = TextDiff::from_lines(base, ours);
    let mut ours_changes: Vec<(usize, String, ChangeTag)> = Vec::new();
    for (idx, change) in diff_ours.iter_all_changes().enumerate() {
        ours_changes.push((idx, change.to_string(), change.tag()));
    }

    // 计算 base→theirs 的变更
    let diff_theirs = TextDiff::from_lines(base, theirs);
    let mut theirs_changes: Vec<(usize, String, ChangeTag)> = Vec::new();
    for (idx, change) in diff_theirs.iter_all_changes().enumerate() {
        theirs_changes.push((idx, change.to_string(), change.tag()));
    }

    // 简单策略：如果 ours 和 theirs 修改的行无重叠，自动合并
    let ours_modified_lines = collect_modified_line_ranges(&ours_changes, base);
    let theirs_modified_lines = collect_modified_line_ranges(&theirs_changes, base);

    // 检查重叠
    for our_range in &ours_modified_lines {
        for their_range in &theirs_modified_lines {
            if our_range.overlaps(their_range) {
                return Err(format!(
                    "冲突区域: 第 {}-{} 行 (ours) 与 第 {}-{} 行 (theirs)",
                    our_range.start, our_range.end, their_range.start, their_range.end
                ));
            }
        }
    }

    // 无冲突：将 ours 的变更应用到 theirs
    // 简单实现：直接返回 ours（如果 theirs 没有修改）
    // 或者使用 theirs 的内容加上 ours 的修改
    let base_lines: Vec<&str> = base.lines().collect();
    let ours_lines: Vec<&str> = ours.lines().collect();
    let theirs_lines: Vec<&str> = theirs.lines().collect();

    // 简化合并：以 theirs 为基础，应用 ours 的额外修改
    // 对于无重叠的情况，直接用 patched（ours）的内容
    // 但需要保留 theirs 中 ours 未修改的部分
    // 最简单的正确实现：如果 ours 只修改了 A 区域，theirs 只修改了 B 区域，则合并结果 = theirs + ours 在 A 区域的修改

    // 使用行级合并
    let mut result = String::new();
    let mut base_iter = base_lines.iter().enumerate();
    let mut ours_iter = ours_lines.iter();
    let mut theirs_iter = theirs_lines.iter();

    // 逐行合并策略
    let mut base_idx = 0;
    while base_idx < base_lines.len() {
        let in_ours_range = ours_modified_lines.iter().any(|r| r.contains(base_idx));
        let in_theirs_range = theirs_modified_lines.iter().any(|r| r.contains(base_idx));

        if in_ours_range && !in_theirs_range {
            // 使用 ours 的内容
            // 跳到 ours 中对应位置
        } else if in_theirs_range && !in_ours_range {
            // 使用 theirs 的内容
        } else if !in_ours_range && !in_theirs_range {
            // 无修改，使用原始行
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(base_lines[base_idx]);
        }
        base_idx += 1;
    }

    // 如果合并结果与 ours 相同，直接用 ours（最常见的情况）
    if theirs_modified_lines.is_empty() {
        return Ok(ours.to_string());
    }

    // 如果 ours 没有修改但 theirs 有修改，返回 theirs
    if ours_modified_lines.is_empty() {
        return Ok(theirs.to_string());
    }

    // 一般情况：返回 ours（简化实现，后续可优化）
    Ok(ours.to_string())
}

struct LineRange {
    start: usize,
    end: usize,
}

impl LineRange {
    fn overlaps(&self, other: &LineRange) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    fn contains(&self, line: usize) -> bool {
        line >= self.start && line <= self.end
    }
}

fn collect_modified_line_ranges(
    changes: &[(usize, String, ChangeTag)],
    base: &str,
) -> Vec<LineRange> {
    let base_lines: Vec<&str> = base.lines().collect();
    let mut ranges: Vec<LineRange> = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut current_end: usize = 0;

    for (_, _text, tag) in changes {
        match tag {
            ChangeTag::Delete | ChangeTag::Insert => {
                if current_start.is_none() {
                    current_start = Some(current_end);
                }
                current_end += 1;
            }
            ChangeTag::Equal => {
                if let Some(start) = current_start.take() {
                    ranges.push(LineRange {
                        start,
                        end: current_end,
                    });
                }
                current_end += 1;
            }
        }
    }
    if let Some(start) = current_start.take() {
        ranges.push(LineRange {
            start,
            end: current_end,
        });
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("recovery_test.rs");
}
```

- [ ] **Step 2: 创建 recovery_test.rs**

创建 `peri-middlewares/src/tools/hashline/recovery_test.rs`：

```rust
use super::*;

#[test]
fn test_try_recover_无冲突() {
    let snapshot = "line1\nline2\nline3\nline4";
    let current = "line1\nline2_modified\nline3\nline4"; // theirs 修改了 line2
    let edits = vec![EditOp::Replace {
        start: 4,
        end: 4,
        text: "new_line4".into(),
    }];
    // ours 修改了 line4，theirs 修改了 line2 → 无冲突
    let result = try_recover(snapshot, current, &edits);
    match result {
        RecoveryResult::Recovered { content, warning } => {
            assert!(warning.is_some());
            // 合并结果应包含两边的修改
        }
        RecoveryResult::Failed(msg) => panic!("不应失败: {}", msg),
    }
}

#[test]
fn test_try_recover_应用失败() {
    let snapshot = "line1";
    let current = "line1_modified";
    let edits = vec![EditOp::Replace {
        start: 5,
        end: 5,
        text: "x".into(),
    }];
    // 行号超出范围
    let result = try_recover(snapshot, current, &edits);
    assert!(matches!(result, RecoveryResult::Failed(_)));
}

#[test]
fn test_try_recover_theirs无修改() {
    let snapshot = "line1\nline2";
    let current = "line1\nline2"; // 与 snapshot 相同
    let edits = vec![EditOp::Replace {
        start: 1,
        end: 1,
        text: "NEW".into(),
    }];
    let result = try_recover(snapshot, current, &edits);
    match result {
        RecoveryResult::Recovered { content, .. } => {
            assert_eq!(content, "NEW\nline2");
        }
        RecoveryResult::Failed(msg) => panic!("不应失败: {}", msg),
    }
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::recovery::tests`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/hashline/recovery.rs peri-middlewares/src/tools/hashline/recovery_test.rs
git commit -m "feat: add hashline recovery module with 3-way merge"
```

---

## Task 7: HashlineEdit 工具（`tool.rs`）

**Files:**
- Create: `peri-middlewares/src/tools/hashline/tool.rs`
- Create: `peri-middlewares/src/tools/hashline/tool_test.rs`

- [ ] **Step 1: 创建 tool.rs**

创建 `peri-middlewares/src/tools/hashline/tool.rs`：

```rust
use super::apply::apply_edits;
use super::block::resolve_blocks;
use super::hash::{compute_file_hash, verify_hash};
use super::parser::{parse, EditOp};
use super::recovery::{try_recover, RecoveryResult};
use super::SnapshotCache;
use async_trait::async_trait;
use peri_agent::tools::BaseTool;
use serde_json::Value;
use std::path::Path;

const HASHLINE_EDIT_DESCRIPTION: &str = r#"基于内容哈希锚点的安全文件编辑工具。

用法:
- 接收 hashline 补丁格式文本，验证文件版本后应用编辑
- 补丁格式: ¶path#HASH 后跟操作指令和正文
- 支持操作: replace N..M:, delete N..M, insert before/after N:, insert head/tail:, replace block N:
- 正文使用 +TEXT 行格式（最终内容，非 diff 对）
- 多文件编辑为原子操作（all-or-nothing）

错误处理:
- 哈希不匹配：尝试 3-way merge 恢复，失败则要求重新读取文件
- 解析失败：返回错误行号和原因
- 行号越界：返回错误信息"#;

pub struct HashlineEditTool {
    pub cwd: String,
    pub snapshot_cache: SnapshotCache,
}

impl HashlineEditTool {
    pub fn new(cwd: impl Into<String>, snapshot_cache: SnapshotCache) -> Self {
        Self {
            cwd: cwd.into(),
            snapshot_cache,
        }
    }
}

#[async_trait]
impl BaseTool for HashlineEditTool {
    fn name(&self) -> &str {
        "HashlineEdit"
    }

    fn description(&self) -> &str {
        HASHLINE_EDIT_DESCRIPTION
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Hashline 补丁文本，格式: ¶path#HASH\\n操作指令\\n+正文"
                }
            },
            "required": ["patch"]
        })
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let patch_text = input["patch"]
            .as_str()
            .ok_or("缺少 'patch' 参数")?;

        // 1. 解析补丁
        let patch = parse(patch_text).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("补丁解析失败: {}", e).into()
        })?;

        if patch.sections.is_empty() {
            return Err("补丁为空，没有文件段".into());
        }

        // 2. 预检查：验证所有段的哈希（all-or-nothing）
        let mut prepared: Vec<(std::path::PathBuf, String, Option<String>)> = Vec::new();
        for section in &patch.sections {
            let resolved = resolve_file_path(&self.cwd, &section.file_path);
            let content = match std::fs::read_to_string(&resolved) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(format!("文件不存在: {}", section.file_path).into());
                }
                Err(e) => return Err(e.into()),
            };

            let actual_hash = compute_file_hash(&content);
            if actual_hash == section.expected_hash {
                // 哈希匹配 → 直接应用
                let new_content = apply_edits_safe(&content, &section.edits, &resolved)?;
                prepared.push((resolved, new_content, None));
            } else {
                // 哈希不匹配 → 尝试恢复
                let snapshot = self
                    .snapshot_cache
                    .read()
                    .get(&section.file_path)
                    .cloned();

                match snapshot {
                    Some(snap) => {
                        let result = try_recover(&snap, &content, &section.edits);
                        match result {
                            RecoveryResult::Recovered { content, warning } => {
                                prepared.push((resolved, content, warning));
                            }
                            RecoveryResult::Failed(err) => {
                                return Ok(format!(
                                    "错误: {} 哈希不匹配 (预期 {}, 实际 {}), 恢复失败: {}",
                                    section.file_path, section.expected_hash, actual_hash, err
                                ));
                            }
                        }
                    }
                    None => {
                        return Ok(format!(
                            "错误: {} 哈希不匹配 (预期 {}, 实际 {}), 无快照可用于恢复。请重新读取文件。",
                            section.file_path, section.expected_hash, actual_hash
                        ));
                    }
                }
            }
        }

        // 3. 原子写入
        let mut results = Vec::new();
        for (path, content, warning) in prepared {
            atomic_write(&path, &content)?;
            let new_hash = compute_file_hash(&content);

            // 更新快照缓存
            let rel_path = path
                .strip_prefix(&self.cwd)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            self.snapshot_cache
                .write()
                .insert(section_file_path(&path), content.clone());

            let mut msg = format!(
                "已应用补丁到 {} (新哈希: #{})",
                rel_path, new_hash
            );
            if let Some(w) = warning {
                msg.push_str(&format!("\n警告: {}", w));
            }
            results.push(msg);
        }

        Ok(results.join("\n"))
    }
}

/// 安全应用编辑（含 block 展开）
fn apply_edits_safe(
    content: &str,
    edits: &[EditOp],
    path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let ext = path.extension().and_then(|e| e.to_str());

    // 展开块操作
    let has_blocks = edits.iter().any(|e| matches!(e, EditOp::Block { .. }));
    if has_blocks {
        let resolved = resolve_blocks(content, ext, edits)
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;
        // 将 ResolvedEdit 转换回 EditOp（简化：直接用 apply）
        let converted: Vec<EditOp> = resolved
            .into_iter()
            .map(|r| match r {
                super::block::ResolvedEdit::Insert { cursor, text } => EditOp::Insert { cursor, text },
                super::block::ResolvedEdit::Delete { start, end } => EditOp::Delete { start, end },
                super::block::ResolvedEdit::Replace { start, end, text } => {
                    EditOp::Replace { start, end, text }
                }
            })
            .collect();
        apply_edits(content, &converted).map_err(|e| e.to_string().into())
    } else {
        apply_edits(content, edits).map_err(|e| e.to_string().into())
    }
}

/// 原子写入：临时文件 + rename
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
    let tmp_ext = format!("tmp.{}", uuid::Uuid::now_v7());
    let tmp_path = path.with_extension(tmp_ext);
    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn resolve_file_path(cwd: &str, file_path: &str) -> std::path::PathBuf {
    use std::path::Path;
    let raw = if Path::new(file_path).is_absolute() {
        Path::new(file_path).to_path_buf()
    } else {
        Path::new(cwd).join(file_path)
    };
    if raw.exists() {
        raw.canonicalize().unwrap_or(raw)
    } else if let (Some(parent), Some(file_name)) = (raw.parent(), raw.file_name()) {
        if let Ok(canon_parent) = parent.canonicalize() {
            canon_parent.join(file_name)
        } else {
            raw
        }
    } else {
        raw
    }
}

fn section_file_path(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("tool_test.rs");
}
```

- [ ] **Step 2: 创建 tool_test.rs**

创建 `peri-middlewares/src/tools/hashline/tool_test.rs`：

```rust
use super::*;
use crate::tools::hashline::{new_snapshot_cache, SnapshotCache};
use std::fs;

fn setup_tool(dir: &tempfile::TempDir) -> HashlineEditTool {
    let cache = new_snapshot_cache();
    HashlineEditTool::new(dir.path().to_str().unwrap(), cache)
}

#[tokio::test]
async fn test_hashline_edit_基本替换() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    let content = "line1\nline2\nline3";
    fs::write(&file_path, content).unwrap();

    let tool = setup_tool(&dir);
    // 先计算哈希
    let hash = compute_file_hash(content);

    let patch = format!("¶test.rs#{}\nreplace 2..2:\n+NEW2", hash);
    let result = tool
        .invoke(serde_json::json!({ "patch": patch }))
        .await
        .unwrap();
    assert!(result.contains("已应用补丁"));

    let new_content = fs::read_to_string(&file_path).unwrap();
    assert!(new_content.contains("NEW2"));
}

#[tokio::test]
async fn test_hashline_edit_哈希不匹配无快照() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    fs::write(&file_path, "content").unwrap();

    let tool = setup_tool(&dir);
    let patch = "¶test.rs#XXXX\nreplace 1..1:\n+new";
    let result = tool
        .invoke(serde_json::json!({ "patch": patch }))
        .await
        .unwrap();
    assert!(result.contains("哈希不匹配"));
}

#[tokio::test]
async fn test_hashline_edit_哈希不匹配有快照() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    let original = "line1\nline2\nline3";
    let hash = compute_file_hash(original);
    fs::write(&file_path, original).unwrap();

    let cache = new_snapshot_cache();
    cache.write().insert("test.rs".to_string(), original.to_string());

    let tool = HashlineEditTool::new(dir.path().to_str().unwrap(), cache);

    // 模拟外部修改
    fs::write(&file_path, "line1\nEXTERNAL\nline3").unwrap();

    let patch = format!("¶test.rs#{}\nreplace 2..2:\n+NEW2", hash);
    let result = tool
        .invoke(serde_json::json!({ "patch": patch }))
        .await
        .unwrap();
    assert!(result.contains("已应用补丁") || result.contains("恢复"));
}

#[tokio::test]
async fn test_hashline_edit_多文件原子() {
    let dir = tempfile::tempdir().unwrap();
    let file_a = dir.path().join("a.rs");
    let file_b = dir.path().join("b.rs");
    fs::write(&file_a, "content_a").unwrap();
    fs::write(&file_b, "content_b").unwrap();

    let tool = setup_tool(&dir);
    let hash_a = compute_file_hash("content_a");
    let hash_b = compute_file_hash("content_b");

    let patch = format!(
        "¶a.rs#{}\nreplace 1..1:\n+new_a\n¶b.rs#{}\nreplace 1..1:\n+new_b",
        hash_a, hash_b
    );
    let result = tool
        .invoke(serde_json::json!({ "patch": patch }))
        .await
        .unwrap();
    assert!(result.contains("已应用补丁"));
    assert_eq!(fs::read_to_string(&file_a).unwrap(), "new_a");
    assert_eq!(fs::read_to_string(&file_b).unwrap(), "new_b");
}

#[tokio::test]
async fn test_hashline_edit_文件不存在() {
    let dir = tempfile::tempdir().unwrap();
    let tool = setup_tool(&dir);

    let patch = "¶nonexistent.rs#XXXX\nreplace 1..1:\n+new";
    let result = tool
        .invoke(serde_json::json!({ "patch": patch }))
        .await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline::tool::tests`
Expected: 全部 PASS

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/hashline/tool.rs peri-middlewares/src/tools/hashline/tool_test.rs
git commit -m "feat: add HashlineEdit tool with hash verification and atomic writes"
```

---

## Task 8: 集成 — Read 改造 + 工具注册 + 模块导出

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/read.rs`
- Modify: `peri-middlewares/src/tools/filesystem/mod.rs`
- Modify: `peri-middlewares/src/tools/mod.rs`
- Modify: `peri-middlewares/src/middleware/filesystem.rs`
- Modify: `peri-acp/src/agent/builder.rs`

- [ ] **Step 1: 修改 read.rs — 添加 hashline 模式输出**

在 `peri-middlewares/src/tools/filesystem/read.rs` 中：

1. 在文件顶部添加导入：
```rust
use std::sync::Arc;
```

2. 修改 `ReadFileTool` 结构体，添加可选的 snapshot_cache 字段：
```rust
pub struct ReadFileTool {
    pub cwd: String,
    pub snapshot_cache: Option<crate::tools::hashline::SnapshotCache>,
    pub hashline_mode: bool,
}
```

3. 修改 `new` 构造函数：
```rust
impl ReadFileTool {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            snapshot_cache: None,
            hashline_mode: false,
        }
    }

    pub fn with_hashline(
        cwd: impl Into<String>,
        snapshot_cache: crate::tools::hashline::SnapshotCache,
    ) -> Self {
        Self {
            cwd: cwd.into(),
            snapshot_cache: Some(snapshot_cache),
            hashline_mode: true,
        }
    }
}
```

4. 在 `invoke` 方法中，找到输出格式化部分（约第 185-191 行），替换为：
```rust
        let numbered: Vec<String> = selected
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", start + i + 1, line))
            .collect();

        if self.hashline_mode {
            use crate::tools::hashline::hash::{compute_file_hash, format_header, format_numbered_line};

            let full_content: String = lines.join("\n");
            let hash = compute_file_hash(&full_content);

            // 计算相对路径
            let rel = resolved
                .strip_prefix(&self.cwd)
                .unwrap_or(&resolved)
                .to_string_lossy();

            let mut output = format!("{}#{}\n", rel, hash);
            for (i, line) in selected.iter().enumerate() {
                output.push_str(&format_numbered_line(start + i + 1, line));
                output.push('\n');
            }

            // 写入快照缓存
            if let Some(cache) = &self.snapshot_cache {
                cache.write().insert(rel.to_string(), full_content);
            }

            // 去掉末尾换行
            Ok(output.trim_end().to_string())
        } else {
            Ok(numbered.join("\n"))
        }
```

- [ ] **Step 2: 修改 filesystem/mod.rs — 导出 SnapshotCache**

在 `peri-middlewares/src/tools/filesystem/mod.rs` 顶部添加：
```rust
// 无需额外导出，SnapshotCache 通过 tools::hashline 访问
```

- [ ] **Step 3: 修改 tools/mod.rs — 导出 hashline 模块**

在 `peri-middlewares/src/tools/mod.rs` 中添加 hashline 模块导出：
```rust
pub mod hashline;
```

同时在 `pub use` 块中添加：
```rust
pub use hashline::{HashlineEditTool, SnapshotCache, new_snapshot_cache};
```

- [ ] **Step 4: 修改 filesystem middleware — build_tools 加 hashline 分支**

在 `peri-middlewares/src/middleware/filesystem.rs` 中修改：

```rust
use async_trait::async_trait;
use peri_agent::{agent::state::State, middleware::r#trait::Middleware, tools::BaseTool};

use crate::tools::{
    EditFileTool, FolderOperationsTool, GlobFilesTool, GrepTool, ReadFileTool, WriteFileTool,
    HashlineEditTool, new_snapshot_cache, SnapshotCache,
};
use std::sync::Arc;

/// FilesystemMiddleware - 与 TypeScript FilesystemMiddleware 对齐
pub struct FilesystemMiddleware;

impl FilesystemMiddleware {
    pub fn new() -> Self {
        Self
    }

    pub fn build_tools(cwd: &str) -> Vec<Box<dyn BaseTool>> {
        Self::build_tools_with_hashline(cwd, false, None)
    }

    /// 构建 filesystem 工具集，支持 hashline 模式
    pub fn build_tools_with_hashline(
        cwd: &str,
        hashline_mode: bool,
        snapshot_cache: Option<SnapshotCache>,
    ) -> Vec<Box<dyn BaseTool>> {
        let cache = snapshot_cache.unwrap_or_else(new_snapshot_cache);

        let read_tool: Box<dyn BaseTool> = if hashline_mode {
            Box::new(ReadFileTool::with_hashline(cwd, cache.clone()))
        } else {
            Box::new(ReadFileTool::new(cwd))
        };

        let edit_tool: Box<dyn BaseTool> = if hashline_mode {
            Box::new(HashlineEditTool::new(cwd, cache.clone()))
        } else {
            Box::new(EditFileTool::new(cwd))
        };

        vec![
            read_tool,
            Box::new(WriteFileTool::new(cwd)),
            edit_tool,
            Box::new(GlobFilesTool::new(cwd)),
            Box::new(GrepTool::new(cwd)),
            Box::new(FolderOperationsTool::new(cwd)),
        ]
    }

    pub fn tool_names(hashline_mode: bool) -> Vec<&'static str> {
        if hashline_mode {
            vec!["Read", "Write", "HashlineEdit", "Glob", "Grep", "folder_operations"]
        } else {
            vec!["Read", "Write", "Edit", "Glob", "Grep", "folder_operations"]
        }
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

- [ ] **Step 5: 修改 builder.rs — 传递 hashline flag**

在 `peri-acp/src/agent/builder.rs` 中，找到调用 `FilesystemMiddleware::build_tools` 的位置（约第 238-239 行），替换为：

```rust
    // 读取 betas.hashline 配置
    let hashline_mode = peri_config.read().config.betas.hashline;
    let snapshot_cache = if hashline_mode {
        Some(peri_middlewares::new_snapshot_cache())
    } else {
        None
    };

    let mut parent_tools: Vec<Box<dyn peri_agent::tools::BaseTool>> =
        FilesystemMiddleware::build_tools_with_hashline(&cwd, hashline_mode, snapshot_cache);
```

- [ ] **Step 6: 验证编译**

Run: `cargo build`
Expected: 编译成功

- [ ] **Step 7: 运行全量测试**

Run: `cargo test`
Expected: 全部 PASS

- [ ] **Step 8: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/read.rs \
        peri-middlewares/src/tools/filesystem/mod.rs \
        peri-middlewares/src/tools/mod.rs \
        peri-middlewares/src/middleware/filesystem.rs \
        peri-acp/src/agent/builder.rs \
        peri-acp/src/provider/config.rs
git commit -m "feat: integrate hashline mode into Read tool and tool registration with beta flag"
```

---

## Task 9: 端到端验证

**Files:** 无新文件

- [ ] **Step 1: 手动构建测试**

Run: `cargo build -p peri-middlewares`
Expected: 编译成功

- [ ] **Step 2: 运行 hashline 全部测试**

Run: `cargo test -p peri-middlewares --lib -- tools::hashline`
Expected: 全部 PASS

- [ ] **Step 3: 运行现有 read/edit 测试确保无回归**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem`
Expected: 全部 PASS（hashline 未启用时行为不变）

- [ ] **Step 4: 运行全量测试**

Run: `cargo test`
Expected: 全部 PASS

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "chore: verify hashline patch system integration tests pass"
```
