# Peri Code 的 Prompt Cache 优化实录：从 20% 到 98.5%

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

缓存命中率代表的是省钱省时间。命中率 20%，每一轮对话都在重新发送同样的 8000 token 系统提示词；命中率 98.5%，这部分 token 费用接近零。

这篇记的是 Peri 怎么一步一步从前者爬到后者的。不是预先设计的，是每次出问题修完之后提炼出来的。

## 为什么缓存会失效

Anthropic 的 Prompt Cache 工作方式是前缀匹配：如果当前请求的开头 N 个 token 和上一次请求完全一致，这部分就命中缓存，不重复计费、不重新推理。

"完全一致"是硬条件。一个字符不同，前缀就断了，后面的全部失效。

## 第一个坑：动态占位符

系统提示词里有两行：

```
当前目录：/Users/.../my-project
今天是：2026-05-13
```

日期每天变，路径换个项目就变。这两个字段混在系统提示词里，把整段 8000 token 的静态内容全带跑了——前缀每次都不一样，缓存完全失效。

修法是引入边界标记 `__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__`。标记前是静态内容（工具说明、行为规范、安全约束，01-06 段落），这部分跨请求、跨天、跨项目完全稳定；标记后是动态内容（日期、cwd、中间件注入），每次请求允许变化。两块分开发给 Anthropic，静态块带 `cache_control`，动态块不带。

## 第二个坑：系统提示词每轮重建

早期每轮 prompt 都重新执行一次 `build_system_prompt()`，把 CLAUDE.md、技能摘要、日期、当前目录重新拼一遍。即使内容没变，重建本身有随机性——中间件注入顺序可能微小漂移。

修法是 `frozen_system_prompt`：在 `session/new` 时构建一次，结果冻结进 `SessionState`，后续所有轮次直接复用。系统提示词在会话内变成真正不可变的。

## 第三个坑：工具列表顺序不稳定

Rust 的 `HashMap` 迭代顺序是随机的——每次进程重启，工具列表的序列化顺序可能不同。API 请求的 `tools` 数组是缓存前缀的一部分，顺序变了，前缀就断了。

修法：工具列表按名称字母序排序，保证每次序列化结果相同。同时把 `ToolSearchIndex` 从每轮重建的局部变量提升为 session 级共享的 `Arc`——不只是为了缓存，也省了重复构建的开销。

## 第四个坑：Skill Preload 破坏消息边界

`SkillPreloadMiddleware` 加载技能文件时，用 `prepend_message` 把合成消息插到消息数组的 index 0。

问题是 `insert(0)` 把所有已有消息右移。Anthropic 的缓存断点打在最后一条 user 消息的前一条——这个位置随着右移漂移，命中率垮掉。

改成 `add_message` 尾部追加，消息边界稳定了，断点不再漂移。

## 第五个坑：system prompt 没有断点

System prompt 的最后一个 block 没有 `cache_control` 标记。Anthropic 只缓存有标记的前缀——没标记的部分，内容再稳定也不缓存。

修法是序列化 system prompt 时，对最后一个 block 加 `cache_control: ephemeral`。这样整个 system prompt，包括 CLAUDE.md 和 Skills 注入的内容，都进了缓存区。

同时发现 `tools` 数组上之前加的 `cache_control` 是冗余的——第一条 user 消息的断点已经隐式覆盖了 tools。去掉那个冗余标记，断点数量减少，缓存结构更清晰。

## 第六个坑：动态区域被打上了断点

代码里有一段 `i == last_idx` 的 fallback 逻辑：遍历 system blocks 时，最后一个 block 无条件加 `cache_control`。

问题是当 MCP 连接状态不同时，动态区域（边界标记之后）内容会变化——比如 MCP 工具注册条目不同。`i == last_idx` 的 fallback 恰好落在动态 block 上，把不稳定的内容加进了缓存断点，静态前缀的缓存就跟着失效。

动态区域的内容本来就不应该缓存。把那段 fallback 逻辑去掉，只让静态区域的最后一个 block 带断点。

## 客户端做完了，还有厂商的问题

客户端六个坑修完，命中率从 20% 涨到 98.5%。但 98.5% 不是天花板，有时候会莫名掉下来——这部分不是代码的问题，是厂商侧的。

**有些 API 根本没有缓存设计。** 部分国内兼容 OpenAI 协议的中转服务从未实现 Prompt Cache，接受请求、返回结果，缓存字段永远是 0。挑 provider 时先确认缓存是否真的有效。

**高峰时期缓存驱逐。** Anthropic 的缓存是 TTL 制，5 分钟不命中就驱逐。高峰时段服务器压力大，缓存驱逐会提前发生。命中率在晚高峰掉下来不一定是代码问题，是 provider 端在丢缓存。

**中转站换号。** 用第三方中转服务时，背后可能有多个 API Key 轮转。Anthropic 的缓存是 per-key 的——今天这个请求走 Key A 建了缓存，下一个请求路由到 Key B，缓存完全失效，相当于每次都是冷启动。这种情况下客户端做再多优化也没用，表现上是随机命中/不命中。

这也是我们推荐 DeepSeek 官方 API 的原因——直连官方、单 key 绑定、缓存驱逐少见，在客户端优化到位之后，命中率能稳得最久。

## 结果

命中率从 20% 涨到 98.5%。每次修完一个问题，就把根因和通用模式写进 CLAUDE.md 的 TRAP 列表。下次 Agent 遇到同类问题，TRAP 是硬约束，自动绕过。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
