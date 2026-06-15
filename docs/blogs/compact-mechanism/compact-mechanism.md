# Peri Agent 如何让长任务跑几个小时不中断——自动上下文压缩机制

我们有一次让 Peri 分析一批 Langfuse 的 trace 日志，大概 300 条 session 记录。Agent 读文件、跑 bash 查询、写中间结论，跑了 40 分钟，到 session 的第 80% 时——`400 Bad Request，context length exceeded`。任务从头来过。

有人会说，现在 DeepSeek V4 Pro、Gemini 1.5 已经支持 1M 上下文了，够用了吧。够用是够用，但这些模型在上下文填到一定程度之后，推理质量会悄悄下降——指令开始被忽略，前面写过的约束不再遵守，回答变得越来越泛。这不是模型的 bug，是超长上下文下注意力机制的结构性限制。窗口再大，有效注意力是有上限的。

所以 compact 不只是为了「塞得下」，是为了让 Agent 在任何时候都工作在注意力有效的区间里，跑几个小时的任务也不退化。

简单截断旧消息不行——AI 做任务靠的是上下文里留着的信息，截掉等于失忆。Peri 的选择是压缩，不是删除。但「压缩」这件事本身有两种完全不同的设计模式，针对不同的触发时机。

## 让 Agent 跑几个小时不中断

Micro-compact 和 Full compact 都发生在 ReAct 循环的 `before_model` 钩子里——每次调用 LLM 之前检查一次，触发了就地压缩，压缩完接着跑，Agent 不感知中断。

一个几小时的长任务，上下文会反复触碰 70% 和 85% 这两条线。Micro-compact 在 70% 时把旧工具输出压掉，上下文降回低水位，任务继续跑；再涨到 85% 触发 Full compact，生成结构化摘要替换整个历史，再次降回低水位。这个循环可以反复发生——每次 compact 都是一次重置，但 Agent 的执行状态完整保留在摘要里。

关键是——这整个过程 Agent 不停，用户不介入。从用户角度看，Peri 在持续输出进展；从 Agent 角度看，它只是在下一轮调用 LLM 之前，上下文变短了一些，但任务目标、已完成的工作、当前状态全在摘要里，它知道自己在做什么，接着做就是了。

对比两种常见的处理方式——任务跑满直接崩，或者让用户手动 `/compact` 介入——两者都要求人介入。Peri 的 compact 不需要人，是 Agent 自己管理自己的上下文寿命。

我们拿 Peri 自己跑长任务验证过这一点。让它做一次大型重构——重新设计消息管线、迁移十几个文件、补测试——整个任务跑了将近 3 小时，触发了 2 次 Full compact、4 次 Micro-compact，任务最终完成，中间没有中断过一次。每次 compact 后 Agent 能正确继续，是因为 9 段结构化摘要把它做了什么、还要做什么都记录清楚了。

## Micro-compact 的设计模式：外科手术，不动骨架

70% 触发 Micro-compact。这一步的设计原则是——只动工具输出，不碰对话。

为什么只动工具输出？因为对话是任务的骨架，工具输出是执行的副产品。一个 bash 结果、一次文件读取，Agent 参考了就扔，留着只是占位置。而对话里的「用户说了什么」「Agent 做了什么决策」，这些才是不能丢的。

具体来说，Micro-compact 有一个白名单——Bash、Read、Glob、Grep、Write、Edit 这六类工具的输出可以压缩。白名单之外的不动。图片是特殊处理，无论哪类工具产出的图片，直接替换成 `[image]`——图片内容在上下文里本来就没法复用，留着纯粹占位置。

时间衰减是另一个关键判断——在 70% 触发后，从消息列表尾部向前扫描，只压缩 5 个 tool_use/tool_result 对之前的工具结果，近 5 对不动，因为 Agent 可能还在引用。两个条件是 AND 关系，70% 触发且超过 5 对，才会被压缩：

```
# 压缩前（第 30 步的 bash 结果，已经 25 步前的事了）
[tool_result: bash]
  "total 248\n-rw-r--r-- 1 user group 12483 May 10 invoke.rs\n
   -rw-r--r-- 1 user group  8921 May 10 mod.rs\n
   -rw-r--r-- 1 user group  4201 May 10 stream.rs\n
   ... 共 47 行目录列表 ..."

# 压缩后
[tool_result: bash]
  "[compacted: bash ~340 tokens]"
```

**工具对保护**是 Micro-compact 必须处理的硬约束。API 层要求 `tool_use` 和 `tool_result` 必须成对出现，少一个直接报错。压缩时不能只删 result 留 use，也不能反过来——Peri 在压缩前把所有工具对识别出来，整对处理，不拆散。压缩只针对 result 的 content/text，`tool_use` 的 arguments 字段必须完整保留——这个细节在后面的 re-inject 阶段至关重要。

这个模式的本质是——在不改变对话结构的前提下，把已经没用的执行细节缩掉。Agent 的推理链完整，只是不再带着几十条废弃的目录列表。

## Full compact 的设计模式：重建骨架，用结构替代原文

85% 触发 Full compact。这一步的逻辑完全不同——不是裁剪，是用 LLM 重新理解整个会话，生成一份结构化摘要来替代原文。

为什么需要结构化摘要，而不是让 LLM 自由总结？因为自由总结的质量不稳定——LLM 可能重点讲了技术细节，却漏掉「当前任务目标是什么」；可能把所有报错列了一遍，但没说哪个已经修了。Agent 接着跑的时候，最需要的信息可能恰好就是漏掉的那个。

Peri 的摘要模板固定 9 段，顺序不变：

```
## Primary Request
用户的原始任务目标

## Technical Concepts
涉及的技术概念和背景

## Files
读写过的文件路径

## Errors & Fixes
出现过的报错和修复方式

## Problem Solving
解决问题的思路和关键决策

## User Messages
用户的指令原文（保留原话）

## Pending Tasks
待处理的工作项

## Current Work
当前正在处理的具体内容

## Next Step
下一步计划
```

这 9 段覆盖了 Agent 继续执行所需的全部上下文——知道目标、知道进展、知道坑在哪、知道下一步去哪。顺序也是按重要性排的，先知道在做什么，再知道做了什么。

**PTL（Prompt Too Long）降级重试**解决摘要本身太长的问题。生成完摘要，加上系统提示词和最近对话，如果还是放不下——分级重试，最多 3 次，每次删掉最旧的一批消息组再试。三次都不行才报错停下，不会用一个不完整的摘要继续跑。

## re-inject：压缩完不等于结束

两级 compact 完成后，Peri 做一次 re-inject——把最近读取过的文件路径和当前激活的 Skills 重新注入上下文。

文件路径的提取依赖 `tool_use` 的 arguments 字段——Read/Write/Edit 工具的调用参数里记录了操作的文件路径。这就是为什么前面 Micro-compact 的工具对保护必须只压缩 result 那侧，arguments 必须完整保留。如果 arguments 丢了，re-inject 阶段就提取不到准确的文件路径，注入的是空内容——Agent 接着跑，不知道最近在操作哪些文件。我们早期踩过这个坑，修复之后把它固化成了硬规则。

Skills 的 re-inject 是因为系统提示词在会话内冻结，中途不可变。压缩后的上下文里如果没有 Skills 内容，Agent 找不到它能用的工具和指令，必须重新注入。

两件事加起来，是让 compact 之后的 Agent 知道「我有哪些工具」，也知道「我在操作哪些文件」——能无缝接着跑，不是一个失忆的新 Agent。

---

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
