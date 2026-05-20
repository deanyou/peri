# System Prompt 每轮重复注入导致上下文膨胀

**状态**：Open
**优先级**：高
**创建日期**：2026-05-20

## 问题描述

Agent 每轮 LLM 调用时，system prompt 中的 "## Deferred Tools" 段落（含 MCP 工具描述）被完整追加而非替换。随着轮次增加，system prompt 呈倍数膨胀：Round 8 为 69K chars（1 份），Round 9 为 145K（2 份），Round 12 为 374K（5 份），Round 55 为 451K（6 份）。每份副本精确 40,259 chars，内容完全相同。这导致即使 LLM 每轮只输出几十到几百 tokens，输入 tokens 仍以每轮 +20~45K 的速度暴涨。

## 症状详情

### System Prompt 倍数膨胀

Session `019e4522-693e-71d1-a67a-86e1ede116bf`，模型 `deepseek-v4-pro`。

| 轮次 | sys[1] 总大小 | "Deferred Tools" 副本数 | 输入 tokens | LLM 输出 tokens |
|------|-------------|----------------------|-----------|----------------|
| 8 | 69K chars | **1** | 43,953 | 283 |
| 9 | 145K chars | **2** | ?（无 stream.log） | ? |
| 11 | 298K chars | **4** | 112,208 | 269 |
| 12 | 374K chars | **5** | 134,967 | 335 |
| 55 | 451K chars | **6** | 170,886 | — |

关键观察：

- **LLM 输出极小**（58-335 tokens），不是膨胀的来源
- **消息内容正常**（每轮 +2 条小消息），也不是来源
- **唯一膨胀来源是 sys[1]**：每轮多一份 40,259 chars 的 "## Deferred Tools" 完整副本
- 副本位置间隔精确相同（0, 40259, 80518, 120777, 161036, 201295），说明是同一段内容被机械式追加

### 被重复的内容

每份 40,259 chars 的副本包含：
- "## Deferred Tools" 段落（Sentry MCP 工具描述：Run Seer、Analyze issue、releases、attachments 等约 20 个子项）
- 后续的 CLAUDE.md 内容（项目概述、依赖关系、架构要点、编码规范等全部中文段落）

所有 section headers 均出现相同次数：`## 项目概述`、`## 依赖关系`、`## 架构要点`、`## 编码规范` 等，与 "Deferred Tools" 副本数一致。

### 输入 Token 暴涨轨迹

| 轮次 | 输入 tokens | 增量 | 说明 |
|------|-----------|------|------|
| 1 | 29,153 | — | 初始（1 份 prompt） |
| 8 | 43,953 | — | 仍为 1 份 prompt（正常增长来自消息累积） |
| 10 | 89,547 | **+45,594** | prompt 变为 2 份（+40K chars ≈ +11K tokens） |
| 11 | 112,208 | **+22,661** | prompt 变为 4 份 |
| 12 | 134,967 | **+22,759** | prompt 变为 5 份 |
| 34 | 161,830 | — | 稳定（prompt 副本数不再增加，仅消息缓慢增长） |
| 55 | 170,886 | — | 最终触发 full compact |

### 缓存状态

缓存命中率 95.6%（主 session），缓存本身工作正常。但因为 sys[1] 的 cache_control 断点内容在每轮变化（追加新副本），缓存命中的是旧的前缀，新增的副本部分为冷 miss。随着副本数增加，缓存效率逐步下降。

### Round 9 缺失 stream.log

Round 9（`2026-05-20_11-29-30-322_0648`）只有 `request.json`，没有 `stream.log`。该轮是 prompt 从 1 份变为 2 份的转折点（用户发送了 `/compact`），可能与响应异常有关。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启用 MCP 工具（如 Sentry MCP）
  2. 使用任意模型进行多轮对话
  3. 每轮观察 system prompt 中 "## Deferred Tools" 段落是否增多
- **环境**：观察到此问题的 session 使用 `deepseek-v4-pro`，但问题可能在所有模型上存在

## 次要问题

以下问题在同 session 中也被观察到，但不是上下文膨胀的主要原因：

- **Micro-compact 未触发**：55 轮、110 条消息期间 micro-compact 一次都没触发
- **子 Agent Read 膨胀**：子 Agent 在 4 轮内从 37K 飙到 100K（并发 Read 大文件，工具结果无截断）
- **Thinking 累积**：每轮 reasoning_content 累积在消息中无法清理

## 相关 Issue

- `spec/issues/2026-05-20-auto-compact-empty-messages-400.md` — Auto compact 后 messages 为空导致 400 错误
- `spec/issues/2026-05-20-compact-command-not-triggering.md` — `/compact` 命令未触发压缩

## 涉及文件

- `peri-acp/src/session/executor.rs` — Agent 执行管线，每轮构建 agent 和 system prompt
- `peri-acp/src/agent/builder.rs` — `build_agent()` 构建 agent，调用 `with_system_prompt()`
- `peri-middlewares/src/mcp/` — MCP 中间件，注册 MCP 工具（"Deferred Tools" 内容的来源）
- `peri-middlewares/src/skills.rs` — SkillsMiddleware，注入 skill 摘要到 system prompt
- `peri-agent/src/agent/react.rs` — ReAct 循环，`with_system_prompt()` 方法
- `peri-tui/prompts/sections/` — System prompt 段落文件
