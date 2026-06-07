# LineEdit 工具在连续编辑场景中频繁产生困惑与副产物

**状态**：Open
**优先级**：低
**创建日期**：2026-06-06

## 问题描述

在本次 bg-agent 修复过程中，使用 LineEdit 对 `message_pipeline/mod.rs` 和 `agent_events_bg.rs` 进行连续编辑时，频繁遇到以下问题，导致原本 ~4 次编辑能完成的工作膨胀到 ~15 次 Read + ~20 次 LineEdit。

## 症状

| 症状 | 表现 |
|------|------|
| 插入不替换 | `insert:true` 在已有函数体上方插入新版本，旧版本未被删除，产生两个 `notify_bg_completed` 函数定义 |
| word 匹配失败 | 3 次用 `start_word`/`end_word` 定位均失败。原因：前次编辑改变注释对齐、缩进空格数变化、编辑范围跨多行时词边界不确定 |
| 行号漂移 | 每次编辑后行号变化，后续编辑需反复 Read 确认位置 |
| 编辑不完整 | 有几次 LineEdit 只替换了匹配词所在行的一部分，剩下大部分旧代码仍在（如 `end_word` 未定位到正确结尾） |
| 副产物堆积 | 修改 `drain_subagent_stack` 尾巴时的插入操作未补闭合 `}`，导致 bracket mismatch，又需要额外一轮修复 |

## 根因

1. **插入/替换语义混合**：同一个工具既做 insert 又做 replace，操作人（LLM）难以判断每次调用是追加还是覆盖。`insert: true` 是追加而非替换旧内容的心理模型不够清晰。

2. **word-level 定位脆弱**：`start_word`/`end_word` 的语义（替换范围 **含**锚定词、行内**必须唯一**）恰好是本仓库 `lineedit-prompt-stress-testing` issue 中记录的 LLM 高频出错场景。短词、缩进敏感、跨行范围都易匹配失败。

3. **无原子事务**：连续多次 LineEdit 中间没有回滚机制，一次失败后文件已处于半修改状态，后续错误叠加。

4. **状态不透明**：每次调用后只能获得"插入 N 行"或"替换 N 行"的反馈，无法确认编辑范围是否正确。需要额外 Read 验证。

## 改进方向

- 在 `insert:true` 时提醒"不会删除已有代码"
- 考虑增加 `replace: true` 显式语义，与 `insert` 互斥
- word-level 匹配失败时返回更具体的错误信息（如"找到 3 处匹配"）
- 对大范围替换（>20 行），引导使用整行 `start_line` + `end_line` 而非 word 锚定

## 关联

- `spec/archive-issues/2026-06-06-lineedit-prompt-stress-testing.md` —— LineEdit 提示词压力测试，记录了 word-level 语义的 LLM 易出错性

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 基于 bg-agent 修复过程中的实际体验创建 |
