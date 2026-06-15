# Peri Code: Issue 归档机制——从 55 个积压记录到一张可检索经验网

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

我们的 issue 目录里堆着 55 个文件，状态字段停在半年前，多数实际已修但标记没改。散落记录 agent 翻不到——修过的坑等于没修过。

我们把这套清理流程固化成了方法——用 agent 把分散的修复记录叠成三层可检索结构，archive 存原文、problems.md 按关键词建索引、CLAUDE.md（agent 的项目记忆文件）的 TRAP 引用指向领域附录里的具体教训。agent 下次碰到关键词，顺着链条就能定位到原始记录。

agent 没有持久记忆，修过的经验不会在会话结束后自动留存。但有了这三层索引，查得到的记录就是它的经验库。

## 状态字段扫描过滤出 3 个可关 Issue

第一步是给 agent 一个入口。`/issue-archive` 就是这个入口——触发后 agent 列出待整理目录下所有文件，用 grep（文本搜索工具）扫描每个文件的 `**状态**` 字段，把状态是 Fixed、Done、Verified 的挑出来。55 个文件，3 个符合条件。但大量 issue 状态没改、代码已经修了——光靠扫状态字段不够，得把没标记但实际已修的都翻出来。

## Agent 并行验证 issue 状态并批量更新

我们从最早的文件开始，按日期倒序逐个过。每个 issue 只读标题和前五行，用户在终端判——"1 2 删除。4 删除。6 检查有无实现。10 修复了。11 重新验证。12 修复。13 修复。14 完成。15 完成。"——七条指令同时执行：删文件的、改状态字段的、派子 agent 去代码库查验证的。没有交互停顿。

用户要求检查验证结果时，子 agent 的输出不是模糊判断。每条验证结论都附带具体的文件路径和行号——HookEvent 没有 PostToolBatch 变体，Write 工具有 append 参数且带 7 个测试，compact 路径里有 `delete_messages` 和 `append_messages` 两个持久化调用。能证实的就标记 Fixed，没实现的原样留着。

18 条验证里，8 条代码确实修了只是没改状态，5 条根本没动过——五个 Hook 相关 issue 从五月底放到现在一行代码没写。还有 3 条修了一半，比如 SQLite 的 `list_child_threads` 和 `list_session_threads` 已经改成 `THREAD_META_COLUMNS` 不再加载 `cached_context`，但 `load_context` 里的 `.clone()` 调用在设计限制下无法消除。

有个 issue 记录了子 Agent 构建时 `parent_tools` 遗漏 WebMiddleware 工具的问题。子 agent grep 了 `agent/builder.rs`、`subagent/tool/build_agent.rs`、`bg.rs` 等五个文件，证实五条路径全漏了；然后又查到 `WebMiddleware::build_tools()` 已经是统一入口，确认修复完成。

七条完全废弃的 issue 直接删除——弹窗光标移动滚动不跟随、ACP 未实现 authenticate 加 logout、长对话内存持续增长。这些问题要么已被后续重构覆盖，要么优先级太低不值得长期挂着。

两轮下来，55 个 issue 缩减至 17 个待处理。删了 7 个，标记 Fixed 和 Done 了 26 个。

## 归档文件头部自动插入签收日期与原始路径

标记完后 agent 把 Fixed、Done 的 issue 从 `spec/issues/` 统一搬到 `spec/archive-issues/`。每个文件头部插一行 `归档于日期，原路径 xxx`。

第一轮搬了 3 个，第二轮搬了 27 个——两轮总共 30 个文件进入 archive。从 active 区到历史区，需要的时候还能翻。

## 从归档 Issue 中提炼通用模式并按领域写入附录

归档文件放进 archive 只是存档。值钱的是从中提炼出的通用模式，按领域写进了领域附录文件——agent 每次执行都会读取的 `spec/global/domains/` 下的 agent.md、tui.md 等。

比如 LLM 流式错误导致 Agent 失忆。具体问题是 executor 中 `?` 传播——Rust 的错误传播操作符，遇到错误立即返回——导致 `cleanup_prepended`（清理注入消息的函数）被跳过，system 消息泄漏到 state。提炼之后——

> history 保护条件过窄，不应仅对 Cancelled 保历史，应改为有进展则保留

并发 BG Agent 后 prompt 卡死，背后是 Langfuse（遥测平台）的 flush 操作阻塞了 agent 的主事件分发循环（event pump）——

> 外部遥测不得阻塞核心事件泵

new_thread 死锁加配置不一致，同一个根因——配置更新先持久化再验证，验证失败后文件已经脏了——

> 配置更新应先验证再持久化

本轮从 30 个归档 issue 中提炼出 18 条通用认知，agent 领域 10 条、tui 领域 2 条、compact 领域 3 条，分别写进对应领域文件。每条格式固定——摘要、状态、关键词、问题本质、通用模式、技术决策——都从 issue 原文抽象出设计教训。agent 执行任务时，CLAUDE.md 指向 domain 文件，domain 文件指向经验附录，形成一条从当前问题到历史教训的检索链。

## 关键词索引覆盖 200 个入口供 agent 检索历史教训

`spec/global/problems.md` 是关键词索引。每个领域认知里的关键词——append 模式、block_in_place 死锁、遥测死锁、PredictionReady——被建一个 `### <关键词>` 标题，下面挂链接指向 domain 文件里的具体条目。

为什么用 Markdown 标题做索引而不是 YAML 或独立 JSON？domain 文件本身已经是 Markdown——用同一种格式、同一个文件体系，agent 不需要切换解析器就能同时读懂索引和正文。新增一条关键词就是加一行 `###` 和几条链接，零学习成本。

本轮新增了 55 个关键词。加上已有的一百多个，现在 problems.md 里有将近两百个关键词入口。agent 在思考问题时搜一个关键词就能翻到历史上所有相关事故。

## CLAUDE.md 补入流式错误失忆的 TRAP 引用链

CLAUDE.md 是 agent 每次执行必读的项目记忆文件，容量有限，全是 `[TRAP]` 标记——只有放下次必定再犯的问题，一条都不多。

本轮加了一条——`spec/global/domains/agent.md#issue_2026-05-29-llm-stream-error-causes-amnesia`，挂在已有 `[TRAP]` Cancel 后历史不应无条件截断后面。LLM 流式错误导致失忆跟 Cancel 截断的根因一样——都是 history 保护条件过窄。补一条链接够了，不用重复写一段。

其他的 block_in_place 死锁、Langfuse 阻塞 event pump 也是重要教训，但 CLAUDE.md 里没有对应段可以挂。容量有限，只放能嵌入现有结构的东西。

55 个 issue 缩减至 17 个待处理。散落的修复记录被拆成了三层——archive 存原文、problems.md 按关键词建索引、CLAUDE.md 的 TRAP 引用指向领域附录里的具体教训。agent 下次碰到关键词，顺着链条就能定位到原始记录。

这套方法不绑特定工具。需要的只是一套 agent 能读的文件结构、一种提取通用模式的固定模板、以及一个关键词索引文件——剩下的扫描、判定、搬运、提炼全交给 agent 自己执行。

agent 没有持久记忆，但有了这些检索链，查得到的记录就是它的经验库。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
