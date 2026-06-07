# LineEdit 成功后高重读率：tool_result 缺少编辑区域上下文导致不必要的 Read

**状态**：Open
**优先级**：中
**创建日期**：2026-06-07

## 问题描述

LineEdit 成功后，Agent 有 68.4% 的概率在 5 步内 Read 回同一文件（对比 Edit 仅 38.6%）。通过 31 个会话的采样分析发现，排除失败后重试（39%，属于 bracket 误报的独立问题），成功后的重读中 **~70% 是因为 tool_result 信息不足导致 Agent 必须读回文件**。

核心原因是行号漂移：LineEdit 用行号定位，编辑后行号变化，后续编辑必须 Read 获取新行号。而当前 tool_result 只返回统计摘要，不包含修改后的行号范围或代码上下文。

## 症状详情

### 重读率数据

| 工具 | 有效编辑数 | 5步内重读同一文件 | 重读率 |
|------|-----------|-----------------|--------|
| LineEdit | 114 | 78 | **68.4%** |
| Edit | 83 | 32 | 38.6% |
| Write | 131 | 52 | 39.7% |

### 成功后重读原因分布（采样 31 个会话）

| 原因 | 占比 | 说明 |
|------|------|------|
| 继续编辑同文件 | ~35% | 行号漂移后必须 Read 获取新行号 |
| tool_result 信息不足 | ~17% | 无法确认修改结果，Read 验证 |
| 确认后编译验证 | ~7% | 大规模重构后重读 + cargo build |
| 纯惯性重读 | 0% | 无 |

### 当前 tool_result 格式（`line_edit.rs:351-393`）

```
✓ /path/to/file.rs (sanity:ok brackets:ok ast:ok)
  3 hunks applied (26 additions, 2 deletions)

1 files, 3 hunks (26+, 2-)
```

**缺失信息**：
- 修改后每个 hunk 的新行号范围
- 修改区域的代码上下文（前后几行）
- 具体替换了什么内容

### 典型案例

**案例 1：继续编辑（行号漂移）**

Agent 在 `hitl/mod.rs` 添加 `BROKER_TIMEOUT` 常量（3 hunks, 26+2-）。成功后立即 Read 同一文件，因为需要在 struct 中加 `broker_timeout` 字段——但插入 26 行后行号已变，tool_result 不提供新行号，必须 Read。

**案例 2：信息不足确认**

Agent 修改 `persistence.rs`（删 32 行加 2 行，1 hunk）。大规模重构后 tool_result 只说 `1 hunks applied (2+, 32-)`，Agent 不确定改对了没有，Read 确认后 `cargo build`。

### 新旧格式对比

LineEdit 有两种参数格式（edits 旧 / patches 新），patches 格式 tool_result 信息量更大，重读率已从 69% 降至 40%，证明信息量是关键杠杆。

## 涉及文件

- `peri-middlewares/src/tools/filesystem/line_edit.rs`（`format_results` 函数，第 351-393 行）—— tool_result 格式化逻辑

## 改进方向

1. **在 tool_result 中返回编辑区域的上下文**：每个 hunk 返回新行号范围 + 前后各 5 行代码。预估可消除 ~70% 的成功后重读
2. **具体格式建议**：

```
✓ /path/to/file.rs (sanity:ok brackets:ok ast:ok)
  3 hunks applied (26+, 2-)
  @@ L16: const BROKER_TIMEOUT: Duration = ...
  @@ L89: broker_timeout: Duration,
  @@ L120-L125: fn new(..., broker_timeout: Duration) -> Self {
```

## 关联

- `spec/issues/2026-06-06-lineedit-consecutive-edits-confusion.md` —— 连续编辑场景的行号漂移问题
- `spec/issues/2026-06-06-lineedit-bracket-false-positive.md` —— bracket 误报导致失败重读（已 Fixed）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 基于 31 个会话的采样分析创建 |
