# SQLite 内存峰值问题（cached_context 全量加载）

**状态**：Partial
**优先级**：中
**类型**：性能
**创建日期**：2026-06-05

## 问题描述

SQLite 存储层存在多处全量加载 `cached_context`（完整消息历史 JSON，~0.5-3MB/线程）的路径，导致内存峰值与线程数成正比。此前 `list_threads()` 已修复（`THREAD_META_COLUMNS` 将 `cached_context` 替换为 `NULL`），但 `list_child_threads()` 和 `list_session_threads()` 仍使用 `THREAD_COLUMNS`（含 `cached_context`）。

## 症状详情

### 现象 1：`list_child_threads()` / `list_session_threads()` 含 `cached_context`

两个方法使用 `THREAD_COLUMNS`（含 `cached_context`），会加载所有子线程的完整消息历史 JSON。

| 调用方法 | 当前调用方 | 峰值估算 |
|----------|-----------|---------|
| `list_child_threads()` | 仅测试 | N × 1-3MB |
| `list_session_threads()` | 仅测试 | N × 1-3MB |

10 个子线程 = 10-30MB 峰值，与之前 `list_threads()` 的问题完全相同。

### 现象 2：`load_context()` 反序列化峰值（会话恢复时）

`load_context()` 命中 `cached_context` 缓存时（`sqlite_store.rs:471-472`）：

```rust
if let Some(json) = cached {
    let mut cached_msgs: Vec<BaseMessage> = serde_json::from_str(&json)?;
```

反序列化瞬间同时持有 JSON 字符串 + `Vec<BaseMessage>`，单线程峰值 ~1.5-5MB。加上 `open_thread()` 中 3 次复制（`origin_messages` + `pipeline.restore_completed` + ACP `load_session` 再次 `load_context`），单次会话恢复峰值 ~6-15MB。

### 现象 3：删除数据库后重启内存更低

清理 SQLite 后重启内存低 <50MB，不是泄漏，而是空库无会话可恢复，不触发 `load_context()` 路径。

## 已实施修复

### 修复 #1（2026-06-05）：`list_child/session_threads` 改用 `THREAD_META_COLUMNS`

- **操作人**：agent
- **修复内容**：将 `list_child_threads()` 和 `list_session_threads()` 的 SQL 从 `THREAD_COLUMNS` 改为 `THREAD_META_COLUMNS`，排除 `cached_context` 大字段
- **涉及文件**：`peri-agent/src/thread/sqlite_store.rs:531,570`
- **验证状态**：已验证（15 个 sqlite_store 测试全部通过）

## 残余问题

`load_context()` 的反序列化峰值（现象 2）属于设计如此（恢复会话需要全量消息），无法避免。可通过以下方式降低：

- `open_thread()` 中消除 `base_msgs.clone()`（`thread_ops.rs:123`、`140`），改用 `std::mem::take`
- 减少消息三重存储（`origin_messages` + `pipeline.completed` + `ACP state.history`）

## 涉及文件

- `peri-agent/src/thread/sqlite_store.rs` — `THREAD_META_COLUMNS`、`THREAD_COLUMNS`、`list_child_threads()`、`list_session_threads()`
- `peri-tui/src/app/thread_ops.rs:108-140` — `open_thread()` 消息三重存储
- `peri-acp/src/dispatch/session_load.rs:13-18` — ACP 侧 `load_context()` 调用

## 关联 Issue

- `spec/issues/2026-06-01-thread-browser-full-table-scan-high-memory.md` — 同类问题，`list_threads()` 已修复
- `spec/issues/2026-05-22-memory-linear-growth-no-compact.md` — 内存线性增长主 issue（Open）
- `spec/issues/2026-05-31-subagent-memory-doubling.md` — SubAgent 内存翻倍（Open）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-05 | — | Open | agent | 创建 |
| 2026-06-05 | Open | Partial | agent | 修复 list_child/session_threads 峰值，load_context 峰值为设计限制 |

## 修复记录

### 修复 #1（2026-06-05）

- **操作人**：agent
- **用户原意**：消除 SQLite 列表查询加载 `cached_context` 导致的内存峰值
- **修复内容**：`list_child_threads()` 和 `list_session_threads()` 改用 `THREAD_META_COLUMNS`（不含 `cached_context`）
- **涉及 commit**：待提交
- **验证状态**：已验证（15/15 测试通过）
