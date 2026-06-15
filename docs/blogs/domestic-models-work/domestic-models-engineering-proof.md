# Peri Code 的 99%，是国产模型写的

[Peri Code](https://github.com/konghayao/peri) 是我们团队用 Rust 写的 Coding Agent，99% 的代码是国产模型生成的。

翻开 git 历史，`Co-Authored-By: deepseek-v4-pro` 和 `Co-Authored-By: glm-5.1`——满眼都是这两个名字。不是因为 Claude 不好用，是因为我们想亲眼确认一件事——在真正的工程项目里，国产模型到底能不能扛起来。

答案是能。但这个「能」，是踩了一堆坑之后得出的结论，不是 benchmark 排行榜上的数字。

## 让国产模型干什么

先说清楚我们在让国产模型做什么。Peri 是一个有完整 Agent 循环的 Coding Agent 框架——ReAct 循环、工具调用、多轮对话、并发 SubAgent，都在里面。代码总量已经上万行，用的是 Rust，类型系统严格、并发安全约束多，随便写两行都得过编译器这关。

这不是让模型写几个函数，是让它持续参与一个真实项目的演进。每一个 feature，每一个 bug fix，代码主要由 DeepSeek-v4-pro 或 GLM-5.1 生成，我们来做 review 和架构决策。

## 踩的坑，才是真实评测

国产模型能用，这个结论是踩坑之后才站住的。让它写 Hello World 不叫评测，叫演示。

真实项目里，我们踩了 4 个让人印象深刻的坑——每一个都跟模型能力无关，都是 API 层的差异。

**GLM 的 tool_result id 问题**

工具调用是 Agent 的核心能力。每次调用工具，模型会返回一个「工具调用请求」，我们执行完毕之后，要把结果打包成「工具结果消息」发回去，让模型继续推理。

Anthropic 的规范里，这个工具结果消息没有 `id` 字段。但 GLM 的兼容端口不管这个——它对每条消息统一去读 `.id` 属性。没有就崩。多轮工具调用时，GLM 网关直接返回 500 错误。

修复方向：给工具结果消息补一个 `id` 字段，没有就自动生成一个 UUID。两行代码，但要找到这个问题，得先把 GLM 接进来跑起来，观察到它在多轮调用里必崩，再一层层追到协议层的差异。这个 fix，是 GLM-5.1 自己帮我们找到的——模型修了自己的 API 兼容问题，有点意思。

**GLM 的 reasoning 字段命名**

DeepSeek 系列模型有「思考链」——在给出答案之前，它会先思考一段，把这段思考过程也返回来，让下一轮请求能看到。OpenAI 规范里这个字段叫 `reasoning_content`，GLM 用的是 `reasoning`——差了 8 个字符。

结果就是 Peri 的解析代码完全忽略了 GLM 的思考过程，首轮的推理链就被丢掉了，跨轮次无法回传，模型越跑越短路。

加一行兼容代码，同时检查两个字段名，解决。问题是先得发现「GLM 的推理结果消失了」，再定位到字段名差异，再验证修复——整个流程得走一遍，不是一眼看出来的。

**DeepSeek V4 的 thinking block 400**

DeepSeek V4 的 API 不接受消息历史里的 thinking 块——它只认纯文本，thinking 内容要走另一个专用字段。但 Peri 的 Anthropic 路径是完整支持 thinking 块的，切到 DeepSeek 之后，历史消息里带着旧的 thinking 块，发过去就是 400 报错。

而且这两个操作必须同时做：过滤掉 content 里的 thinking 块，同时把思考文本放进顶层 `reasoning_content` 字段一起发回去。缺一个，要么 400，要么模型推理链断掉。这个 fix 的根因分析和方案，是 DeepSeek-v4-pro 给出的——commit `90c51d4c`。

**Kimi 的 thinking/reasoning_effort 互斥**

Kimi k2.6 有一个其他 provider 都没有的限制：`thinking`（开启深度推理）和 `reasoning_effort`（控制推理强度）这两个参数，不能同时出现在请求里，同时发就是 400。

标准 OpenAI 规范里这两个参数完全可以共存。我们加了一个判断：检测到模型名含 `kimi` 且开启了 thinking 时，把 `reasoning_effort` 字段从请求里摘掉。一行代码，但这个问题在 Kimi 文档里找不到明确说明，只有真的跑起来才踩得到。

## 这些坑，说明不了能力问题

上面 4 个坑，一个都不是模型代码写得差。

GLM 的字段命名是 API 设计选择，DeepSeek 的 thinking 格式是协议差异，Kimi 的参数互斥是文档没说清楚——这是 API 层的现实，不是模型推理能力的问题。任何一个刚接入的新 provider 都会踩类似的坑，包括 Anthropic 自己。Anthropic 的 extended thinking 我们也踩过——为缺少 thinking 块的历史消息注入占位 block，同样是一个只有接进来跑才会发现的问题。

真正衡量模型能力的，是拿到任务之后给出的代码能不能跑通。

Peri 这个 Rust 项目的复杂度不低。ReAct 循环的错误处理、多工具并发的结果收集、消息缓存的前缀稳定性、会话持久化的幂等写入——这些逻辑，DeepSeek-v4-pro 和 GLM-5.1 都参与了生成，而且都跑通了。跑通的意思是：过了 Rust 编译器，过了单元测试，在真实会话里没有崩。

## 多 provider 适配层的意义

为什么要做这么多 per-provider 的适配？因为国产模型的 API 和 OpenAI 规范之间，有大量没文档化的差异。

`stream_options.include_usage` 只有 Qwen 需要。`reasoning` vs `reasoning_content` 字段名 GLM 和 DeepSeek 不一样。Kimi 有参数互斥约束。DeepSeek V4 的 content 数组格式有限制。每一个 provider 都有自己的小脾气。

这不是槽点。OpenAI 生态也是踩了无数遍坑、文档化了、第三方工具适配了，才到今天这个状态。国产模型的生态积累是少一截——但这是时间问题，不是能力问题。

Peri 把这些适配全部收进了 LLM 适配层的 per-provider 分支里，上层 Agent 代码感知不到任何差异。换 DeepSeek 还是 GLM，`Ctrl+T` 切一下，agent 继续跑，工具调用继续用，推理链继续传。

## 国产模型写国产 Agent 这件事

到现在，Peri 的 99% 代码由国产模型生成，这件事本身已经说明了很多。

不是「国产模型也能写代码」，是「国产模型在写一个真实生产级 Rust Agent 框架」。踩坑、修坑、继续跑——这个循环一直在转，Peri 现在已经是自己修自己了。

模型能不能用，不是看 benchmark，是看这个。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
