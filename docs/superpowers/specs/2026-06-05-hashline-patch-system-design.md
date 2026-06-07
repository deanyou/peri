# Hashline 补丁编辑系统设计

> 基于 oh-my-pi hashline 的安全文件编辑工具，用内容哈希锚点替代传统行号定位。

## 概述

在 `peri-middlewares` 中新增 `src/tools/hashline/` 模块，实现基于内容哈希的补丁编辑工具（HashlineEdit），替代现有的 Edit 工具。通过 `settings.json` 的 `betas.hashline` flag 控制，启用后 Read 工具输出格式也随之改变。

### 设计目标

- **安全性**：内容哈希验证防止过时文件被损坏
- **原子性**：多文件补丁 all-or-nothing
- **恢复能力**：3-way merge 处理外部编辑导致的漂移
- **向后兼容**：beta flag 关闭时行为完全不变

### 模块结构

```
peri-middlewares/src/tools/hashline/
├── mod.rs       — 模块入口 + flag 检查 + 工具注册
├── hash.rs      — 哈希计算与文本归一化（纯函数）
├── parser.rs    — 补丁格式 tokenizer + parser
├── apply.rs     — 编辑应用算法（纯函数）
├── recovery.rs  — 3-way merge 恢复
├── block.rs     — tree-sitter block 操作解析
└── tool.rs      — HashlineEdit 工具（BaseTool 实现）
```

---

## §1 哈希锚点机制（`hash.rs`）

纯函数模块，零 IO，零额外依赖。

### 归一化策略

与 oh-my-pi 保持一致：裁剪尾部空白（空格/Tab/CR）+ 统一 LF 换行。

```rust
fn normalize(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end_matches(|c| c == ' ' || c == '\t' || c == '\r'))
        .collect::<Vec<_>>()
        .join("\n")
}
```

### 哈希计算

使用 `std::collections::hash_map::DefaultHasher`，取低 16 位生成 4 字符大写十六进制标签（如 `#A3F2`）。

```rust
pub fn compute_file_hash(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let normalized = normalize(text);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    let low16 = hasher.finish() & 0xFFFF;
    format!("{:04X}", low16)
}
```

**设计决策**：`DefaultHasher` 不保证跨版本稳定，但这里只需要同一进程内一致性（Read 计算的哈希在 Edit 时验证）。4 字符十六进制在同一会话内足够区分文件版本。

### 公开 API

```rust
pub fn compute_file_hash(text: &str) -> String;
pub fn verify_hash(text: &str, expected: &str) -> bool;
pub fn format_header(path: &str, hash: &str) -> String;    // "path#HASH"
pub fn format_numbered_line(line_num: usize, text: &str) -> String; // "     1\tTEXT"
```

---

## §2 补丁格式解析（`parser.rs`）

两阶段：Tokenizer（逐行分类）→ Parser（Token 流 → `Patch`）。

### 类型定义

```rust
pub struct Anchor {
    pub line: usize, // 1-indexed
}

pub enum Cursor {
    Bof,
    Eof,
    BeforeAnchor(Anchor),
    AfterAnchor(Anchor),
}

pub enum EditOp {
    Insert { cursor: Cursor, text: String },
    Delete { start: usize, end: usize },       // [start, end], 1-indexed, inclusive
    Replace { start: usize, end: usize, text: String },
    Block { anchor_line: usize, text: String }, // 延迟展开
}

pub struct PatchSection {
    pub file_path: String,
    pub expected_hash: String,
    pub edits: Vec<EditOp>,
}

pub struct Patch {
    pub sections: Vec<PatchSection>,
}
```

### Token 类型

| Kind | 示例 |
|------|------|
| Header | `¶src/main.rs#A3F2` |
| OpReplace | `replace 5..7:` |
| OpDelete | `delete 3..5` |
| OpInsertBefore | `insert before 2:` |
| OpInsertAfter | `insert after 4:` |
| OpInsertHead | `insert head:` |
| OpInsertTail | `insert tail:` |
| OpBlock | `replace block 1:` |
| Payload | `+    new line` |

### 解析规则

- 每个 Header token 开始一个新 `PatchSection`
- 操作 token 后收集连续的 Payload 行，去掉 `+` 前缀，用 `\n` 连接为 `text`
- Replace/Block 的正文可以为空（等同于 Delete）
- 多个操作可跟在同一个 Header 下（同一文件多次编辑）
- 解析失败返回结构化错误（行号 + 原因）

---

## §3 编辑应用算法（`apply.rs`）

纯函数模块，零 IO。将 `Vec<EditOp>` 应用到文本内容。

### 应用流程

1. **展开 Block**：调用 `block.rs` 将 Block 操作转换为具体行范围的 Replace
2. **分区**：将编辑分为 bof-inserts、eof-inserts、anchor-targeted 三类
3. **排序**：anchor 编辑自底向上处理（保持行号有效性）
4. **应用**：逐个执行 splice 操作
5. **拼接**：bof_inserts + modified_lines + eof_inserts

### 自底向上排序

所有 anchor-targeted 编辑按锚点行号从大到小排序。这样前面的编辑不会影响后面编辑的行号。

### 边界修复（`repair_boundaries`）

移植 oh-my-pi 的边界修复逻辑，检测 LLM 是否在正文中重复了未变更的边界行：

1. 检测正文是否复述了未变更的边界行
2. 计算跨行分隔符平衡 `{ (, [, { }`
3. 丢弃导致不平衡的重复结构闭符号（如 `}`, `]);`）
4. 保留有意的重复和平衡内容

---

## §4 恢复机制（`recovery.rs`）

哈希不匹配时，从对话历史提取快照执行 3-way merge。

### 快照提取

通过 Read 工具和 HashlineEdit 工具共享的 `Arc<RwLock<HashMap<PathBuf, String>>>` 快照缓存获取。Read 工具读取文件时自动将完整内容写入缓存，HashlineEdit 工具在哈希不匹配时从缓存中取快照。

```rust
pub type SnapshotCache = Arc<RwLock<HashMap<String, String>>>;
```

**降级策略**：缓存中找不到（如 LRU 淘汰或首次编辑未读取）时直接拒绝，让 LLM 重新读取文件。

### 3-way Merge 流程

```rust
pub enum RecoveryResult {
    Recovered { content: String, warning: Option<String> },
    Failed(String),
}

pub fn try_recover(
    snapshot: &str,      // 从历史提取的文件内容
    current: &str,       // 磁盘当前内容
    edits: &[EditOp],    // LLM 生成的编辑
) -> RecoveryResult
```

1. 将编辑应用到快照版本 → 得到 `patched`
2. 3-way merge：`base=snapshot`, `ours=patched`, `theirs=current`
3. 无冲突 → 返回合并内容 + 警告
4. 有冲突 → 返回失败信息，引导 LLM 重新读取

### 3-way Merge 实现

基于 `similar` crate。需在 `peri-middlewares/Cargo.toml` 中新增 `similar` 依赖（workspace 级统一版本）。

---

## §5 Block 操作（`block.rs`）

将 `replace block N:` 延迟展开为具体行范围的 `Replace`。

### Tree-sitter 集成

```rust
pub fn resolve_block_edits(
    content: &[&str],
    edits: &[EditOp],
) -> Result<Vec<ResolvedEdit>, BlockError>
```

1. 根据文件扩展名选择 tree-sitter 语言
2. 解析 AST
3. 找到覆盖 anchor_line 的最小语法节点
4. 返回该节点的行范围 `[start, end]`（1-indexed）

### 支持的语言（初始版本）

- Rust（`tree-sitter-rust`）
- TypeScript/TSX（`tree-sitter-typescript`）
- JavaScript/JSX（`tree-sitter-javascript`）
- Python（`tree-sitter-python`）
- Go（`tree-sitter-go`）

### 回退策略

不支持的语言或解析失败时，回退到简单缩进块检测：从 anchor_line 开始，按缩进级别确定块的结束行。

### 新增依赖

```toml
[dependencies]
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-python = "0.23"
tree-sitter-go = "0.23"
```

---

## §6 HashlineEdit 工具 + Read 改造

### HashlineEdit 工具（`tool.rs`）

实现 `BaseTool` trait，参数为单个 `patch` 字符串字段。

**关键约束**：`BaseTool::invoke()` 签名为 `async fn invoke(&self, input: Value) -> Result<String, ...>`，无法直接访问 `state.messages()`。恢复机制需要通过以下方式之一获取消息历史：

- **方案 A（推荐）**：在 `HashlineEditTool` 中维护一个 `Arc<RwLock<HashMap<PathBuf, String>>>` 快照缓存，Read 工具在同一缓存中写入。工具间共享状态。
- **方案 B**：在 tool dispatch 层将 `state.messages()` 序列化后通过 input 传递（侵入性大，不推荐）。

选择方案 A：Read 工具读取文件时将完整内容写入共享快照缓存，HashlineEdit 工具从缓存中取快照。缓存 miss 时降级为拒绝。

**执行流程**：

1. **解析**：`parse(patch_text)` → `Patch`
2. **预检查**：遍历所有段，读取文件并验证哈希（all-or-nothing）
3. **恢复**：哈希不匹配时调用 `try_recover()`
4. **写入**：所有段预检查通过后，原子写入（临时文件 + rename）
5. **返回**：每段的路径 + 新哈希 + 可选警告

### Read 工具改造

Beta flag 启用时，输出格式增加哈希头部：

```
¶src/main.rs#A3F2
     1  use std::io;
     2
     3  fn main() {
     4      println!("hello");
     5  }
```

在 `read.rs` 的输出格式化阶段加分支，flag 关闭时输出格式完全不变。

### Beta Flag 机制

- 配置路径：`settings.json` → `betas.hashline`（布尔值）
- 启用时：Read 输出带哈希头 + Edit 替换为 HashlineEdit
- 未启用时：行为完全不变，零影响
- 工具注册点在 `peri-middlewares` 的 `build_tools()` 中根据 flag 切换

---

## 数据流总结

```
Read 工具（hashline mode）
    ↓ 读取文件 → compute_file_hash() → 输出 ¶path#HASH + 行号内容
    ↓
LLM 生成补丁
    ↓ "¶path#A3F2\nreplace 5..7:\n+new line"
    ↓
HashlineEdit 工具
    ↓ parse() → Patch { sections, edits }
    ↓ 预检查：read_to_string → compute_file_hash → verify_hash
    ├─ 匹配 → apply_edits() → 原子写入
    └─ 不匹配 → find_snapshot_from_history() → try_recover()
                                          ↓ apply_edits(snapshot)
                                          ↓ three_way_merge()
                                          ↓ 原子写入 + 警告
```

---

## 与现有系统的关系

| 组件 | 变更类型 | 说明 |
|------|---------|------|
| `peri-middlewares/src/tools/hashline/` | 新增 | 完整 hashline 模块（7 个文件） |
| `peri-middlewares/src/tools/filesystem/read.rs` | 修改 | 输出格式加 hashline 分支 + 快照缓存写入 |
| `peri-middlewares/src/tools/filesystem/mod.rs` | 修改 | 工具注册加 flag 切换 + 共享快照缓存初始化 |
| `settings.json` schema | 修改 | 新增 `betas.hashline` 字段 |
| `Cargo.toml` | 修改 | 新增 `similar` + tree-sitter 系列依赖 |

不变的部分：Edit 工具本身保留（beta flag 关闭时继续使用）、所有其他工具不变、ACP/TUI 层不变。
