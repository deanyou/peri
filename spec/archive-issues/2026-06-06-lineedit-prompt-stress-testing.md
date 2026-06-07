> 归档于 2026-06-06，原路径 spec/issues/2026-06-06-lineedit-prompt-stress-testing.md

# LineEdit 提示词压力测试方法论

**状态**：Closed
**优先级**：低
**创建日期**：2026-06-06

## 问题描述

LineEdit 工具的 `start_word`/`end_word` 语义复杂（替换范围=锚定词起始→结束、行内必须唯一、缺 end_word 报错等），需要验证 LLM 在一次调用中正确执行多种编辑的能力，并迭代优化工具提示词（`LINE_EDIT_DESCRIPTION`）和用户提示词策略。

## 测试样本

`prompts/lineedit_stress_test.txt` —— 包含以下陷阱场景：

- `第二行：HelloWorldHello` 重复 3 次 → start_word/end_word 唯一性挑战
- `第四行：这是第四行` 重复 5 次 → CJK 无空格文本唯一性挑战
- 第10行空行 → insert 方向混淆
- 文件首/尾行 → 边界条件
- Emoji、Tab、前导/后置空格行 → 复杂字符

## 测试任务（5 个编辑）

| # | 编辑 | 类型 |
|---|------|------|
| A | 最后一行 → `BOUNDARY：尾行替换成功` | 整行替换 |
| B | 第四行前3个 `这是第四行` → `【CJK替换成功】` | start_word/end_word 行内替换 |
| C | 第二行第一个 `HelloWorldHello` → `【替换成功】` | start_word/end_word 行内替换 |
| D | 第10行空行后插入新行 | insert |
| E | 首个样本标题行整行替换 | 整行替换 |

## 6 轮迭代历史

| 轮次 | 成功率 | 重试 | 关键发现 |
|------|--------|------|----------|
| 1 | 5/5 | 1 | 发现陷阱 #4（替换范围含锚定词）、#5（缺 end_word→默认到行尾） |
| 2 | 3/5 | - | 未先 Read 文件，行号与内容标签混淆 |
| 3 | 4/5 | 0 | 加 `步骤1：Read` 后行号正确，但 end_word 不唯一时 Agent 问"要重试？"未自动重试 |
| 4 | 5/5 | 1 | 加 `预判唯一性 + 失败自动重试` 指令 |
| 5 | 5/5 | 0 | 步骤式提示词，零重试稳定通过 |
| 6 | 5/5 | 0 | 自然语言 + 最小 Hint，同样零重试稳定 |

## LineEdit 工具提示词（最终版 5 Caution）

```
Caution: new_string replaces the target range entirely — do not duplicate content from adjacent lines outside the edit range.
Caution: start_word/end_word must be unique within the line. If the word matches multiple times, use a longer prefix to disambiguate.
Caution: when replacing an entire line, omit start_word/end_word and use only start_line.
Caution: the replacement range is from START of start_word to END of end_word — not the text between them. Anchor words themselves will be replaced.
Caution: start_word and end_word must both be provided — missing end_word causes an error. To replace to end of line, set end_word to a word near the line tail.
```

## 用户提示词模板（已验证稳定）

**最小 Hint 版**（第6轮验证）：

```
用LineEdit修改prompts/lineedit_stress_test.txt：
- [任务列表]
Hint: 先用Read确认行号，start_word/end_word必须都提供且唯一。
```

**完整步骤版**（第5轮验证）：

```
步骤1：Read 读取 prompts/lineedit_stress_test.txt 获取精确行号。
步骤2：对涉及 start_word/end_word 的编辑，先判断该词在目标行是否唯一，不唯一则扩展前缀。
步骤3：LineEdit 一次调用完成所有编辑（bottom-to-top）。如任何编辑失败，根据错误调整后重试，不要问我。
```

## 关键结论

1. **先 Read 是必须的**：未读文件前 Agent 会将内容标签（如"第10行"）误当文件行号
2. **自动重试指令不可或缺**：Agent 默认在失败后询问用户而非自行修复
3. **CJK 唯一性是最难挑战**：无空格重复文本中短词永远不唯一，需跨越多个重复单元构造长前缀
4. **缺 end_word 从静默替换到行尾改为报错后，Agent 不再在此摔倒**

## 涉及文件

- `peri-middlewares/src/tools/filesystem/line_edit.rs` —— `LINE_EDIT_DESCRIPTION`（5 Caution）
- `prompts/lineedit_stress_test.txt` —— 压力测试样本文件

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Closed | agent | 6 轮迭代完成，工具提示词和用户提示词模板均收敛稳定 |
