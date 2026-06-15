# DeepSeek V4 Pro 编程到底行不行——几百次提交后的体感

> DeepSeek V4（预览版）在编程领域表现如何？最好是真实工作场景下的使用情况。

---

Peri 项目近 29 万行 Rust 代码，4 月 24 日 DeepSeek V4 发布到现在，git 历史里 `deepseek-v4-pro` 的 Co-Authored-By 出现了 251 次。同期 glm-5.1 是 288 次，Claude Opus 4.7 是 180 次。V4 Pro 不是试用了一天的玩具——它是我们的主力模型之一。

## 工具调用没问题

Peri 是 ReAct 循环架构，每轮可能并发调 Read/Grep/Edit/Bash 四五个工具。V4 Pro 的 tool use 格式遵循度稳定——我们专门为并发工具调用的错误处理修过三个 bug（orphaned tool_use、deferred error、串行执行），这些 bug 是框架层面的问题，换哪个模型都会触发，V4 Pro 在修完之后跑得很稳。

DeepSeek 官方后来出了 Claude Code 对接文档，改几个环境变量就能用。说明他们对工具调用场景是认真适配过的。

## thinking 模式是重灾区

三个归档 issue 全跟 thinking 有关，每一个都是 400 错误：

DeepSeek 要求 thinking 模式下所有 assistant 消息必须回传 thinking block，但我们的 SkillPreloadMiddleware 会注入假的 ToolUse 消息——没有 thinking，API 直接拒绝。这个问题在 Anthropic 上不触发，因为 Anthropic 不强制要求回传。

另一个坑：thinking block 不能放在 content 数组里当 `{"type": "thinking"}`，只能用顶层字段。我们把 Reasoning block 序列化到 content 数组里发过去，DeepSeek 直接 400。

还有一个 `prepend_message` 的 `insert(0)` 导致消息重复的问题——所有 provider 都受影响，但 DeepSeek 的 API 校验比 Anthropic 严格，是第一个报错的。

**规律**：在 DeepSeek 上跑通的消息序列，在 Anthropic 上基本不会有协议问题。DeepSeek 可以当协议兼容性的试金石。

## 什么时候切回 Claude

架构层面的决策——比如 ReAct 循环的错误传播路径怎么设计、compact 后消息结构怎么保证不变量——这种需要深度推理的活，V4 Pro 偶尔给「看起来对但跑不通」的方案。同一个任务给 Opus，一步到位的概率更高。

日常编码、功能开发、测试补全、文档，V4 Pro 都够用。67 个 feat、79 个 fix、35 个 refactor——插件市场、工作区配置、面板滚动条、`git-stats` CLI 从零搭建，全是 V4 Pro 写的，没出过大的架构返工。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
