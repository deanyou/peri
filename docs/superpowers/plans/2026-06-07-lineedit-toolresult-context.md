# LineEdit tool_result 增加编辑区域上下文

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 LineEdit 成功的 tool_result 中返回每个 hunk 的新行号范围和上下文代码，消除 Agent 因行号漂移导致的不必要 Read。

**Architecture:** 修改 `FileResult` 结构体，增加 `hunk_details: Vec<HunkDetail>` 字段记录每个 hunk 应用后的新行号范围和上下文。在应用阶段（splice 后）收集这些信息，在 `format_results` 中格式化输出。上下文行数固定为 3 行，总输出限制在 2000 字符内防止上下文膨胀。

**Tech Stack:** Rust, 现有 `line_edit.rs` / `line_edit_diff.rs` 数据结构

---

### Task 1: 扩展 FileResult 结构体，增加 HunkDetail

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit.rs:47-54`

- [ ] **Step 1: 在 FileResult 上方定义 HunkDetail 结构体**

在 `line_edit.rs` 的 `FileResult` 定义上方添加：

```rust
/// 单个 hunk 应用后的位置信息
struct HunkDetail {
    /// hunk 应用后的起始行号（1-based）
    new_start: usize,
    /// hunk 应用后的结束行号（1-based，含）
    new_end: usize,
    /// 上下文行（hunk 范围前后各 3 行，来自修改后的文件）
    context_lines: Vec<String>,
}
```

- [ ] **Step 2: 在 FileResult 中增加 hunk_details 字段**

将 `FileResult` 改为：

```rust
/// 文件应用结果
struct FileResult {
    file_path: String,
    hunk_count: usize,
    additions: usize,
    deletions: usize,
    verify_result: VerifyResult,
    /// 每个 hunk 应用后的位置信息和上下文
    hunk_details: Vec<HunkDetail>,
}
```

- [ ] **Step 3: 运行 cargo check 验证编译**

Run: `cargo check -p peri-middlewares 2>&1 | head -30`
Expected: 编译错误仅在 `FileResult` 构造处（缺少 `hunk_details` 字段），不会有其他问题。

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs
git commit -m "feat(lineedit): add HunkDetail struct for tool_result context"
```

---

### Task 2: 在应用阶段收集 HunkDetail

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit.rs:200-322`

- [ ] **Step 1: 在应用循环中记录每个 hunk 的新行号范围**

在应用阶段（`line_edit.rs` 的 `for mh in &sorted_matched` 循环之后），收集每个 hunk 的信息。

关键点：因为 hunks 从后往前应用（bottom-to-top），前面 hunk 的行号不受后面 hunk 影响。但应用顺序会改变行号，需要在 **全部应用完成后** 统一计算新行号。

在 `sorted_matched` 应用循环之前，记录每个 hunk 的原始匹配位置和替换行数差：

在 `let mut total_additions = 0usize;` 下方增加：

```rust
// 记录每个 hunk 的应用信息（用于事后计算新行号）
// (原始 line_idx, old_count, new_count)
let mut hunk_apply_info: Vec<(usize, usize, usize)> = Vec::new();
```

在 splice 替换之后（`lines.splice(line_idx..end_idx, replacement_lines);` 之后）增加：

```rust
hunk_apply_info.push((line_idx, old_count, replacement_lines.len()));
```

- [ ] **Step 2: 在写入文件前，基于修改后的 lines 计算每个 hunk 的新位置**

在 `let new_content = ...` 之前（验证之前），计算新的行号范围。因为 hunks 从后往前应用，需要按原始 line_idx 排序后累加偏移：

在 `hunk_apply_info` 收集完成后、`verify` 之前，增加计算逻辑：

```rust
// 计算每个 hunk 在修改后文件中的新行号范围
// hunks 按从后往前应用（sorted_matched 已按 Reverse 排序），
// 但 hunk_apply_info 也是从后往前的顺序
// 需要按原始 line_idx 正序排列后累加偏移
let mut sorted_info: Vec<(usize, usize, usize)> = hunk_apply_info.clone();
sorted_info.sort_by_key(|(line_idx, _, _)| *line_idx);

// 计算每个 hunk 的新起始行号
// offset = 所有在此 hunk 之前应用的 hunk 的行数差之和
let mut detail_map: std::collections::HashMap<usize, (usize, usize)> =
    std::collections::HashMap::new(); // orig_line_idx -> (new_start, new_end)
let mut cumulative_offset: isize = 0;
for (orig_idx, old_count, new_count) in &sorted_info {
    let new_start = (*orig_idx as isize + cumulative_offset + 1) as usize; // 1-based
    let new_end = new_start + new_count - 1;
    detail_map.insert(*orig_idx, (new_start, new_end));
    cumulative_offset += *new_count as isize - *old_count as isize;
}

// 构造 HunkDetail
const CONTEXT_LINES: usize = 3;
let hunk_details: Vec<HunkDetail> = sorted_info
    .iter()
    .map(|(orig_idx, _, new_count)| {
        let (new_start, new_end) = detail_map[orig_idx];
        // 上下文范围：前 3 行到后 3 行
        let ctx_start = new_start.saturating_sub(CONTEXT_LINES + 1); // 0-based
        let ctx_end = (new_end + CONTEXT_LINES).min(lines.len()); // 0-based exclusive
        let context_lines: Vec<String> = (ctx_start..ctx_end)
            .map(|i| lines.get(i).cloned().unwrap_or_default())
            .collect();
        HunkDetail {
            new_start,
            new_end,
            context_lines,
        }
    })
    .collect();
```

- [ ] **Step 3: 将 hunk_details 传入 FileResult 构造**

将 `results.push(FileResult { ... })` 改为包含 `hunk_details`：

```rust
results.push(FileResult {
    file_path: patches
        .iter()
        .find(|p| {
            resolve_path(&self.cwd, &p.file_path)
                .to_string_lossy()
                .as_ref()
                == file_key.as_str()
        })
        .map(|p| p.file_path.clone())
        .unwrap_or_else(|| file_key.clone()),
    hunk_count: matched.len(),
    additions: total_additions,
    deletions: total_deletions,
    verify_result,
    hunk_details,
});
```

- [ ] **Step 4: 运行 cargo check 验证编译**

Run: `cargo check -p peri-middlewares 2>&1 | head -30`
Expected: 编译错误仅在 `format_results` 签名未使用 `hunk_details`，不会有逻辑错误。

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs
git commit -m "feat(lineedit): collect hunk position details during apply phase"
```

---

### Task 3: 实现 format_results 输出编辑区域上下文

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit.rs:351-393`

- [ ] **Step 1: 重写 format_results 函数**

将整个 `format_results` 函数替换为：

```rust
fn format_results(results: &[FileResult]) -> String {
    let mut output = Vec::new();
    let mut total_hunks = 0usize;
    let mut total_additions = 0usize;
    let mut total_deletions = 0usize;

    // 输出上限：防止上下文膨胀
    const MAX_OUTPUT_CHARS: usize = 2000;
    let mut output_chars = 0usize;

    for r in results {
        let icon = if r.verify_result.has_error() {
            "✗"
        } else {
            match (&r.verify_result.brackets, &r.verify_result.ast) {
                (VerifyLevel::Warn(_), _) | (_, VerifyLevel::Warn(_)) => "⚠",
                _ => "✓",
            }
        };

        let header = format!(
            "{} {} ({})",
            icon,
            r.file_path,
            r.verify_result.format_tags()
        );
        output.push(header);
        output.push(format!(
            "  {} hunks applied ({}+, {}-)",
            r.hunk_count, r.additions, r.deletions
        ));

        // 每个 hunk 的位置摘要
        for (i, detail) in r.hunk_details.iter().enumerate() {
            if output_chars >= MAX_OUTPUT_CHARS {
                let remaining = r.hunk_details.len() - i;
                if remaining > 0 {
                    output.push(format!(
                        "  ... {} more hunks (output truncated)",
                        remaining
                    ));
                }
                break;
            }

            let range = if detail.new_start == detail.new_end {
                format!("L{}", detail.new_start)
            } else {
                format!("L{}-{}", detail.new_start, detail.new_end)
            };

            // 上下文首行摘要（截断到 80 字符）
            let first_new_line = detail
                .context_lines
                .iter()
                .find(|l| !l.trim().is_empty())
                .map(|l| truncate_str(l.trim(), 60))
                .unwrap_or_default();

            let line = format!("  @@ {}: {}", range, first_new_line);
            output_chars += line.chars().count();
            output.push(line);
        }

        total_hunks += r.hunk_count;
        total_additions += r.additions;
        total_deletions += r.deletions;
    }

    // 汇总行
    output.push(format!(
        "\n{} files, {} hunks ({}+, {}-)",
        results.len(),
        total_hunks,
        total_additions,
        total_deletions
    ));

    output.join("\n")
}
```

- [ ] **Step 2: 运行 cargo check 验证编译**

Run: `cargo check -p peri-middlewares 2>&1 | head -30`
Expected: 编译通过，无错误。

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit.rs
git commit -m "feat(lineedit): output hunk position context in tool_result"
```

---

### Task 4: 更新现有测试断言

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

现有测试通过 `result.contains("✓")` / `result.contains("✗")` 断言成功/失败，这些不受影响。但需要增加新的断言验证 tool_result 包含行号信息。

- [ ] **Step 1: 在 test_单hunk替换 中验证行号输出**

在 `test_单hunk替换` 测试的 `assert_eq!` 之后增加：

```rust
    // 验证 tool_result 包含新行号
    assert!(result.contains("@@ L2:"), "应包含 hunk 新行号: {result}");
```

完整的 `test_单hunk替换` 测试：

```rust
#[tokio::test]
async fn test_单hunk替换() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应标记成功: {result}");
    assert!(result.contains("@@ L2:"), "应包含 hunk 新行号: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nBBB\nccc\n"
    );
}
```

- [ ] **Step 2: 在 test_多hunk同文件 中验证多行号输出**

在 `test_多hunk同文件` 测试的最后一个 `assert_eq!` 之后增加：

```rust
    // 验证 tool_result 包含两个 hunk 的行号
    assert!(result.contains("@@ L2:"), "应包含第一个 hunk 行号: {result}");
    assert!(result.contains("@@ L7:"), "应包含第二个 hunk 行号: {result}");
```

- [ ] **Step 3: 在 test_插入新行 中验证行号范围格式**

在 `test_插入新行` 测试的最后一个 `assert_eq!` 之后增加：

```rust
    // 插入后行号范围应为多行
    assert!(result.contains("@@ L3-L4:"), "插入应扩展行号范围: {result}");
```

- [ ] **Step 4: 运行全部 LineEdit 测试**

Run: `cargo test -p peri-middlewares --lib -- line_edit 2>&1`
Expected: 所有测试通过。

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "test(lineedit): verify hunk position in tool_result"
```

---

### Task 5: 新增测试——验证上下文格式和输出截断

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

- [ ] **Step 1: 添加测试——大文件多 hunk 的输出截断**

在测试文件末尾添加：

```rust
// ─── 大文件多 hunk 输出截断 ──────────────────────────────────────────

#[tokio::test]
async fn test_多hunk输出截断() {
    // 构造 100 行文件，10 个 hunk
    let lines: Vec<String> = (0..100).map(|i| format!("line{}", i)).collect();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("big.txt"), format!("{}\n", lines.join("\n"))).unwrap();
    let tool = make_tool(&dir);

    // 10 个 hunk，每个替换 1 行
    let mut hunks = String::new();
    for i in (0..10).rev() {
        let line_no = i * 10 + 1;
        hunks.push_str(&format!(
            "@@ -{line_no},1 +{line_no},1 @@\n line{}\n+LINE{}\n",
            i * 10, i * 10
        ));
    }
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "big.txt",
                "diff": format!("--- a/big.txt\n+++ b/big.txt\n{hunks}")
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    // 验证包含行号信息
    assert!(result.contains("@@ L"), "应包含行号: {result}");
}

// ─── 单行 hunk 行号格式（非范围） ──────────────────────────────────

#[tokio::test]
async fn test_单行hunk行号格式() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\nddd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -2,1 +2,1 @@\n-bbb\n+BBB"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应成功: {result}");
    // 单行 hunk 应显示 L2 而非 L2-L2
    assert!(result.contains("@@ L2:"), "单行应显示 L2: {result}");
    assert!(!result.contains("L2-L2"), "单行不应显示范围: {result}");
}

// ─── 文件首行 hunk 的上下文不越界 ──────────────────────────────────

#[tokio::test]
async fn test_首行hunk上下文不越界() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,1 +1,1 @@\n-aaa\n+AAA"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应成功: {result}");
    assert!(result.contains("@@ L1:"), "首行 hunk 应显示 L1: {result}");
}
```

- [ ] **Step 2: 运行新增测试**

Run: `cargo test -p peri-middlewares --lib -- line_edit 2>&1`
Expected: 所有测试通过（原有 + 新增）。

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "test(lineedit): add tests for hunk context format and edge cases"
```

---

### Task 6: 全量验证

**Files:** 无新增修改

- [ ] **Step 1: 运行 peri-middlewares 全部测试**

Run: `cargo test -p peri-middlewares 2>&1 | tail -20`
Expected: 所有测试通过。

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy -p peri-middlewares 2>&1 | grep -i warning | head -10`
Expected: 无新增 warning。

- [ ] **Step 3: 运行 fmt check**

Run: `cargo fmt -p peri-middlewares --check 2>&1`
Expected: 无格式差异。

---

## Self-Review

**1. Spec coverage:**
- Issue 要求"返回编辑区域的上下文" → Task 2 收集上下文，Task 3 格式化输出 ✓
- Issue 要求"每个 hunk 返回新行号范围" → HunkDetail.new_start/new_end ✓
- Issue 建议 "@@ L16: const BROKER_TIMEOUT: ..." 格式 → Task 3 实现 ✓
- 输出截断防止上下文膨胀 → Task 3 MAX_OUTPUT_CHARS ✓

**2. Placeholder scan:**
- 所有代码块包含完整实现 ✓
- 无 TBD/TODO ✓
- 无"add appropriate error handling" ✓

**3. Type consistency:**
- HunkDetail 定义在 Task 1，使用在 Task 2/3 ✓
- FileResult 构造在 Task 2 包含所有字段 ✓
- format_results 接收 `&[FileResult]`，与 invoke 中调用一致 ✓
- `truncate_str` 已存在于 `line_edit.rs:343`，直接复用 ✓
