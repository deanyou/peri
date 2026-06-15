# 用 Claude Code 开发复杂功能总返工——我们踩了几百次之后摸出来的流程

> 小白刚学 Claude Code，但开发复杂功能总是返工……感觉自己缺一套结构化的方法论，能把「研究代码库→规划方案→写代码→验证」这几步拆开来做，而不是混在一起搞成一团乱麻。

---

你现在的开发节奏大概是：发现一个需求，跟 Claude 说「帮我实现一下」，它开始写，写到一半你发现理解偏了，改方向，又写到一半发现改坏了另一个模块，再回头修。一个下午过去，git log 里全是 WIP 和 fixup。

我们踩了几百次同样的坑。后来靠两套工具把流程理顺了：[Superpowers 插件](https://www.skills.sh/obra/superpowers)（`writing-plans`、`subagent-driven-development`、`systematic-debugging`）管实现流程，[Peri 项目自建的 issue 循环](https://github.com/konghayao/peri)（`issue-create`、`issue-verify`、`issue-archive`）管问题生命周期。两套工具独立，但串起来刚好覆盖「发现问题→规划→实现→沉淀」全链路。

## 第一步：把问题记下来，不要急着修

遇到 bug 或技术债，第一反应不是让模型直接改，而是 `/issue-create`。

这个 skill 是我们 Peri 项目自建的。它会像记者一样采访你——什么现象、什么条件下触发、涉及哪些文件。只记录症状，不做诊断，不开代码。产出一份结构化的 issue 文档，存到 `spec/issues/` 里。

为什么要记？因为复杂 bug 的根因往往和你第一眼看到的不一样。先记现象，后面诊断的时候有据可查，不会丢上下文。Peri 归档了 192 个 issue，回头看其中很多当初的直觉判断都是错的——真正修的时候发现根因在另一个模块。

## 第二步：写计划，不写代码

复杂功能必须先写实现计划——`/writing-plans`，来自 [Superpowers 插件](https://www.skills.sh/obra/superpowers)。

Superpowers 是 obra 做的一套 Claude Code skills 插件，里面包含 `writing-plans`、`executing-plans`、`subagent-driven-development`、`systematic-debugging` 等十几个 skill。装上就能用，不绑定特定项目。

计划写到什么粒度？每个 step 是一个 2-5 分钟的动作：
- 写失败的测试 → 跑一下确认失败 → 写最小实现 → 跑一下确认通过 → 提交

五个 step，每个都带完整代码和预期输出。没有「TBD」「后续补充」「添加适当的错误处理」这种占位符——计划里写了什么，执行的时候就按什么来。

关键是计划阶段模型只读代码不写代码。它要研究清楚改哪些文件、文件之间的依赖关系、改动顺序。这一步把「理解」和「实现」彻底分开。

Peri 项目里存了 130 个这样的计划，全在 `docs/superpowers/plans/` 里。

## 第三步：subagent 逐任务执行

计划写完后，用 `/subagent-driven-development` 执行——也是 Superpowers 的 skill。

不是一口气全做完——每个任务派一个独立的 subagent。subagent 只看到当前任务的相关上下文，不会被之前任务的残留信息干扰。做完一个任务，过两道 review：

1. **spec review**——改的代码和计划里写的一致吗？有没有多做或少做？
2. **code quality review**——代码本身质量过关吗？

两道都过了才标记完成，进下一个任务。review 不通过就让同一个 subagent 修，修完再 review，循环到过为止。

这比一个人从头干到尾靠谱得多。每个 subagent 上下文干净，review 有独立的质量检查。V4 Pro 参与了 249 次提交（67 个 feat、79 个 fix），插件市场、工作区配置、面板滚动条这些功能全是 subagent 流程跑出来的，没出过大的架构返工。

## 第四步：修完之后沉淀

Bug 修完不是终点。`/issue-archive`（Peri 自建的 skill）把解决的 issue 从 `spec/issues/` 归档到 `spec/archive-issues/`，同时提炼经验教训更新到 `spec/global/domains/` 对应的领域文件里。

Peri 的 CLAUDE.md 里那些 `[TRAP]` 标记——「prepend_message 的 insert(0) 右移会导致 StateSnapshot 索引失效」「DeepSeek 要求 thinking 模式下所有 assistant 消息必须回传 thinking block」——全是从归档 issue 里提炼出来的。下次遇到类似问题，模型直接读到这些约束，不用再踩一遍。

## 完整流程

修 bug：`issue-create`（Peri）→ `systematic-debugging`（Superpowers）→ `writing-plans`（Superpowers）→ `subagent-driven-development`（Superpowers）→ `issue-archive`（Peri）

做新功能：`grill-me`（先跟模型辩论清楚需求）→ `writing-plans`（Superpowers）→ `subagent-driven-development`（Superpowers）

不是每个任务都要走全流程。改一行配置、修个 typo 直接干就行。但只要涉及多文件、多模块、有依赖关系的改动——先计划再动手，返工率会降很多。

Peri 的 issue 循环是项目自建的，Superpowers 是开源插件，两者独立但互补——一个管问题生命周期，一个管实现流程。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
