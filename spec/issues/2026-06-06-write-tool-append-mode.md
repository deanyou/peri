# Write 工具新增 append 模式——支持增量写入降低上下文消耗

**状态**：Open
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

Write 工具当前仅支持全量覆写模式：LLM 必须在 `content` 参数中传入完整文件内容。当文件较长（>200 行）时，`content` 参数在 tool_use input 中占用大量上下文空间（最大观测值 71.1KB / 2367 行，占 128K 上下文的 14.2%），且该内容在消息历史中不可压缩。

缺陷分析数据（`bun src/main.ts --focus write`）显示：
- 44 次 Write 调用中 6 次超过 500 行（13.6%），3 次超过 1000 行
- 大型文件（>200行）平均占用 4.2% 上下文窗口
- 61.4% 的 Write 操作前未 Read（盲写率），多为新建文件场景

需要为 Write 工具新增 `append` 参数，支持 LLM 只传增量内容，降低上下文消耗。

## 症状详情

| 维度 | 数据 |
|------|------|
| Write 总调用 | 44 次 |
| 大文件(>500行) | 6 次 (13.6%) |
| 超大文件(>1000行) | 3 次 (6.8%) |
| 最大单次 | 2367 行 / 71.1KB |
| 大型文件平均上下文占比 | 4.2% (最大 14.2%) |
| 盲写率(Write前未Read) | 61.4% |
| 重复写入同一文件 | 4 次 |

大文件写入集中在 `.md`（计划文档，平均 348 行）和 `.rs`（测试文件，最大 673 行）类型。

## 期望改进方向

在 Write 工具中新增可选参数 `append`（默认 `false`），当 `append=true` 时：
- LLM 只传增量内容（新追加的部分），不传完整文件
- 工具侧用 `std::fs::OpenOptions::new().append(true)` 直接追加到文件末尾
- tool_result 返回 `'Appended N lines to <path> (file total: M lines)'`，不回传文件内容
- 不需要先 Read 原文件，零额外 IO

## 涉及文件

- `peri-middlewares/src/tools/filesystem/write.rs` —— Write 工具实现，需新增 `append` 参数解析和追加写入逻辑
- `peri-middlewares/src/tools/filesystem/write_test.rs` —— 测试文件，需新增 append 模式测试

## 设计要点

1. **参数**：`append` 可选布尔参数，默认 `false`。`true` 时 content 追加到文件末尾而非覆写
2. **写入策略**：`std::fs::OpenOptions::new().append(true).open()` 直接追加，POSIX 保证 O_APPEND 模式下 write() 的原子性
3. **出参格式**：`Appended {n} lines to {rel_path} (file total: {total} lines)`
4. **文件不存在时**：`append=true` 时自动创建文件（`create(true).append(true)`），行为与 `Write` 创建新文件一致
5. **工具描述更新**：在 description 中引导 LLM 对大文件使用 append 模式

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |

## 修复记录
