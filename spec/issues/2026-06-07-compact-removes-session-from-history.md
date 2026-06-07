# Compact 后会话从 History 面板永久消失

**状态**：Open
**优先级**：高
**创建日期**：2026-06-07

## 问题描述

执行 compact（手动 `/compact` 或自动 compact）后，该会话从 TUI 的 History 面板中永久消失，无法通过搜索或浏览找到。重启 TUI 后会话仍然不可见，表现为数据丢失。

## 症状详情

| 现象 | compact 前 | compact 后 |
|------|-----------|-----------|
| History 面板中会话可见 | 正常显示 | 完全消失 |
| 重启 TUI 后会话可见 | 正常显示 | 仍然消失 |
| 搜索关键词定位该会话 | 能搜到 | 搜不到 |
| 会话列表浏览 | 能看到 | 看不到 |

**复现频率**：必现，每次 compact 后都会消失。

## 调查结论（2026-06-07）

### 已排除的假设

**数据库层完整**——直接查询 `~/.peri/threads/threads.db` 确认：

```sql
SELECT id, title, hidden, message_count FROM threads WHERE hidden = 0;
```

7 个执行过 compact 的会话全部存在，`hidden = 0`、`cwd` 正确、`title` 保留原始值、`message_count` 与实际消息数一致。

**compact 持久化路径正确**（`prompt.rs:153-178`）：
- `delete_messages` 只删除 `messages` 表记录，不碰 `threads` 表的 `hidden`/`cwd`/`title`
- `append_messages` 仅在 `title IS NULL` 时设标题（已有标题不受影响）
- 无任何代码路径在 compact 中调用 `delete_thread` 或设置 `hidden = true`

### 最可能的根因方向

**TUI 层渲染/刷新问题**：
1. `open_thread_browser()`（`thread_ops.rs:312-334`）的 `cwd` 过滤可能与实际 `cwd` 不匹配
2. History 面板可能在 compact 事件后未正确刷新列表
3. `ThreadBrowser` 的 `threads` 列表可能在某些事件回调中被错误清空或替换

### 建议的下一步

1. 在 TUI 中实时复现：创建会话 → 对话 → 执行 compact → **不关闭面板** → 观察 History 列表变化
2. 添加 debug 日志到 `open_thread_browser()`：记录 `cwd`、`list_threads()` 返回数量、过滤后数量
3. 检查 compact 事件是否触发了 History 面板的某种刷新/重建逻辑

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 在 TUI 中创建一个新会话
  2. 进行若干轮对话
  3. 执行 compact 操作（手动 `/compact` 或等待自动 compact）
  4. 打开 History 面板搜索该会话 → 找不到
  5. 重启 TUI → 仍然找不到
- **环境**：TUI 模式，所有模型

## 涉及文件

- `peri-tui/src/app/thread_ops.rs` — History 面板加载（`open_thread_browser`），cwd 过滤
- `peri-tui/src/thread/browser.rs` — ThreadBrowser 组件，搜索/过滤/渲染
- `peri-tui/src/acp_server/prompt.rs` — compact 持久化路径（`delete_messages` + `append_messages`）
- `peri-agent/src/thread/sqlite_store.rs` — SQLite 存储实现

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |
| 2026-06-07 | Open | Open | agent | 调查：数据库层完整，问题指向 TUI 层 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
