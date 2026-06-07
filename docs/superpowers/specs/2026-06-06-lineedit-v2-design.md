# LineEdit V2 设计文档

**日期**：2026-06-06
**状态**：Approved
**关联 Issue**：`spec/issues/2026-06-06-lineedit-consecutive-edits-confusion.md`

---

## 1. 问题

LineEdit 工具在连续编辑场景中频繁产生困惑与副产物。根因分析（实现层审计 + 竞品调研 + 压力测试）识别出 12 个缺陷：

| # | 严重度 | 问题 |
|---|--------|------|
| 1 | 致命 | 非原子事务——部分失败后文件半修改 |
| 2 | 致命 | 跨行 word_edit 静默丢弃中间行 |
| 3 | 严重 | `insert`/`replace` 语义混合 |
| 4 | 严重 | `find_unique_word` 用子串匹配非 word boundary |
| 5 | 中 | `start_word`/`end_word` 不对称 |
| 6 | 中 | 同行 `start_col > end_col` 无校验 |
| 7 | 中 | 编辑结果不透明 |
| 8 | 中 | 文件不存在中断所有文件处理 |
| 9 | 低 | 空文件换行符丢失 |
| 10 | 低 | `new_string.lines()` 吞尾部空行 |
| 11 | 低 | 重叠编辑无检测 |
| 12 | 低 | 并发安全无保证 |

## 2. 设计决策

基于 4 个关键设计问答：

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 验证策略 | 警告但继续 | 不阻断编辑流程，LLM 看到警告可自行决定是否 Re-read |
| 原子性 | 全有或全无 | LLM 处理"全失败+重试"远好于"部分成功+猜状态" |
| start_word/end_word | 干净移除 | 压力测试证明不可靠，expected_lines 是更好的替代 |
| 反馈格式 | 上下文 diff | 减少 LLM 额外 Read 调用 |
| 方案选择 | 方案 A：原地改良 | 单一工具避免跨工具原子性问题 |

## 3. 新参数 Schema

```json
{
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
            "description": "1-based line number to start editing at (from Read output)"
          },
          "end_line": {
            "type": "integer",
            "description": "Line number to end editing at. Defaults to start_line. Ignored when action=insert."
          },
          "action": {
            "type": "string",
            "enum": ["replace", "insert", "delete"],
            "description": "Edit action. 'replace' (default): replace lines start_line..end_line with new_string. 'insert': insert new_string before start_line, no lines removed. 'delete': remove lines start_line..end_line, new_string ignored."
          },
          "new_string": {
            "type": "string",
            "description": "Replacement text. For replace: the new content. For insert: content to insert. For delete: ignored, can be empty string."
          },
          "expected_lines": {
            "type": "string",
            "description": "Optional but recommended: content you expect at start_line..end_line from your last Read. If actual content differs, a warning is returned but edit still proceeds."
          }
        },
        "required": ["file_path", "start_line", "new_string"]
      }
    }
  },
  "required": ["edits"]
}
```

**移除的字段**：`insert`、`start_word`、`end_word`——从结构体和 schema 中彻底删除，不做向后兼容。

**`action` 解析逻辑**：
```
action 字段存在 → 使用它
action 缺失 + new_string 为空 → delete
action 缺失 + 其他 → replace
```

## 4. 执行引擎

### 4.1 两阶段执行

```
invoke(input):
  1. 解析 edits → Vec<EditEntry>
  2. 按文件分组（BTreeMap<file_path, Vec<&EditEntry>>）
  3. 每组内按 start_line 降序排列
  4. 阶段 1：validate_edit（只读）
     - 行号范围检查（start_line >= 1, end_line >= start_line, 不超文件长度）
     - 同文件编辑范围重叠检测
     - expected_lines 比对（归一化：trim 尾部空白）
     - 收集结果：Vec<ValidateResult>（Ok 或 Err 或 Warn）
  5. 如果有任何 Err → 不写入任何文件 → 返回全部错误
  6. 阶段 2：apply_edit（写入内存 lines）
     - 按 action 分发：replace / insert / delete
     - 记录变更用于反馈
  7. 逐文件 atomic_write
  8. 构建反馈（含上下文 diff）
```

### 4.2 expected_lines 验证逻辑

```rust
fn verify_expected_lines(
    lines: &[String],
    start_idx: usize,
    end_idx: usize,
    expected: &str,
) -> ExpectedLinesResult {
    let actual: String = lines[start_idx..=end_idx]
        .iter()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    let expected_norm: String = expected
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    if actual == expected_norm {
        ExpectedLinesResult::Match
    } else {
        ExpectedLinesResult::Mismatch {
            expected: expected_norm,
            actual,
        }
    }
}
```

验证结果不影响编辑执行（警告但继续），但会反映在反馈中（⚠ 标记）。

### 4.3 重叠编辑检测

```rust
fn check_overlap(edits: &[&EditEntry]) -> Result<(), Vec<String>> {
    // 对同一文件的编辑（已降序排列），检查任意两个的范围是否重叠
    // (a.start_line..=a.end_line) ∩ (b.start_line..=b.end_line) ≠ ∅
}
```

重叠 → 报错拒绝，不做 best-effort。

### 4.4 apply_edit 分支

**replace**：
```
lines.splice(start_idx..=end_idx, new_lines)
```

**insert**：
```
for (i, line) in new_lines.iter().enumerate() {
    lines.insert(insert_idx + i, line.to_string());
}
```

**delete**：
```
lines.splice(start_idx..=end_idx, std::iter::empty())
```

### 4.5 尾部空行处理

保持现有 `str::lines()` 行为。`"xxx\n".lines()` = `["xxx"]`，只插入一行。这不是 bug——LLM 传 `"xxx\n"` 预期就是一行 `"xxx"`。

## 5. 反馈格式

### 5.1 成功 + 验证通过

```
✓ src/main.rs:42-44 replace (3→3 lines)
  41 | impl Handler for MyService {
  42 |-fn handle(&self, req: Request) -> Result {
  43 |-    let config = self.config.lock().unwrap();
  44 |-    process(req, config)
  42 |+fn handle(&self, input: Input) -> Result {
  43 |+    let opts = self.opts.lock().unwrap();
  44 |+    process(input, opts)
  45 | }
```

### 5.2 成功 + expected_lines 警告

```
⚠ src/main.rs:42-44 replace (3→3 lines) expected_lines 不匹配
  预期: fn handle(&self, req: Request) -> Result {
  实际: fn process(&self, req: Request) -> Result {
  编辑已执行，建议 Re-read 确认结果
  42 |-fn process(&self, req: Request) -> Result {
  42 |+fn handle(&self, input: Input) -> Result {
```

### 5.3 失败（验证阶段拒绝）

```
✗ Edit 1: start_line 99 超出文件行数 (共 42 行)
✗ Edit 2: 第 15-16 行与第 10-12 行重叠，请调整范围
未执行任何编辑。修正后重试。
```

### 5.4 上下文行数规则

- 多行编辑（>1 行）：前后各 2 行
- 单行编辑：前后各 3 行
- 总输出不超过 30 行，超出截断中间并标注 `... (省略 N 行) ...`

## 6. 工具描述

```rust
const LINE_EDIT_DESCRIPTION: &str = r#"Performs precise line-based edits in files.

Line numbers are 1-based (from Read output). Multiple edits are applied bottom-to-top.
All edits in one call are atomic — if any edit fails, no changes are written.

Actions (set "action" field):
- "replace" (default): Replace lines start_line..end_line with new_string.
- "insert": Insert new_string BEFORE start_line. No existing lines are removed.
- "delete": Remove lines start_line..end_line. new_string is ignored, can be "".

Verification with expected_lines (recommended):
- Set to the content you expect at start_line..end_line from your last Read.
- If actual content differs, a warning is returned but the edit still proceeds.
- This catches stale line numbers after concurrent changes.

Rules:
- new_string replaces the ENTIRE target range — do not duplicate adjacent lines.
- For whole-line edits, use start_line/end_line only.
- Multiple edits to the same file must not overlap.

Common patterns:
- Replace lines: {start_line: 42, end_line: 44, expected_lines: "...", new_string: "..."}
- Insert before line: {start_line: 42, action: "insert", new_string: "new line"}
- Delete lines: {start_line: 42, end_line: 44, action: "delete", new_string: ""}
- Single line: {start_line: 42, new_string: "replacement content"}"#;
```

## 7. 关键文件

| 文件 | 改动 |
|------|------|
| `peri-middlewares/src/tools/filesystem/line_edit.rs` | 核心重构：新参数、两阶段引擎、反馈格式、描述重写 |
| `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | 全部测试重写：覆盖 action/expected_lines/原子性/重叠检测 |
| `peri-middlewares/src/middleware/filesystem.rs` | 注册逻辑不变（工具名不变） |
| `peri-middlewares/src/tool_search/core_tools.rs` | TOOL_LINE_EDIT 常量不变 |

## 8. 测试清单

| 测试 | 覆盖场景 |
|------|----------|
| `test_action_replace` | 显式 action:"replace" |
| `test_action_insert` | action:"insert"，不删除旧行 |
| `test_action_delete` | action:"delete"，忽略 new_string |
| `test_action_default_replace` | 无 action 字段，默认 replace |
| `test_action_default_delete` | 无 action + new_string=""，默认 delete |
| `test_expected_lines_match` | 验证匹配，反馈标记 ✓ |
| `test_expected_lines_mismatch_warn` | 验证不匹配，警告但执行，反馈标记 ⚠ |
| `test_expected_lines_trim_trailing` | 尾部空白归一化 |
| `test_expected_lines_multiline` | 多行验证 |
| `test_atomic_all_or_nothing` | 多编辑中一个失败 → 全部不写入 |
| `test_atomic_cross_file` | 跨文件原子性 |
| `test_overlap_detection` | 同文件重叠编辑 → 报错拒绝 |
| `test_no_overlap_different_files` | 不同文件编辑互不影响 |
| `test_feedback_context_diff` | 反馈包含上下文行 |
| `test_feedback_truncation` | 大编辑输出截断 |
| `test_insert_at_beginning` | start_line=1 insert |
| `test_insert_at_end` | start_line=len+1 insert（追加） |
| `test_replace_single_line` | 单行替换 |
| `test_replace_multiline` | 多行替换 |
| `test_replace_full_file` | 替换整个文件 |
| `test_delete_all_lines` | 删除所有行 |
| `test_empty_file_insert` | 空文件中插入 |
| `test_crlf_preserved` | CRLF 换行符保留 |
| `test_line_number_out_of_range` | 行号超出范围 → 报错 |
| `test_file_not_found` | 文件不存在 → 报错 |
| `test_overlap_edge_case` | 首尾相接（start=end+1）不算重叠 |
