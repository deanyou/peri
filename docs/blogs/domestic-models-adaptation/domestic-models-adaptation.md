# Peri Code——OpenAI 兼容中的不兼容

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。

GLM-5.1 返回了一条完全正常的 assistant 消息——reasoning 字段有思考过程，content 字段有回答。Parse 成功，对话继续。下一轮把这条消息原样回传，400 Bad Request。

原因不难找——GLM 要求 reasoning 内容放在 `reasoning_content` 顶层字段回传，不是塞在 content 数组里。它还有另一个叫 `reasoning` 的顶层字段，语义一样，名字不同。

这是我们在适配国产模型时踩的第一个坑，也是最容易修的一个。

## 兼容的边界

DeepSeek、GLM、Qwen、Kimi 都号称 OpenAI 兼容 API。基础对话和工具调用，四家确实兼容得不错——换一个 base URL，改一个 model 名字，跑起来没毛病。

但 Agent 框架需要的远不止基础对话。推理模式的字段回传、流式事件的 token 统计、请求体参数的互斥关系、finish_reason 的非标准值——这些边缘行为上，每家都有自己的理解。文档上写的是一回事，代码里实际接受的才是协议。

Peri Code 现在的处理方式很简单——通用路径走 OpenAI 标准，分歧路径用模型名嗅探做运行时分支。不优雅，但务实。同一个 API 地址可能代理多个模型，用户不需要为每个模型单独配 provider 类型，运行时按模型名分支是目前唯一可行的方案。

## 推理字段，各走各的路

四家模型的推理字段没有两家是一样的。

GLM 用 `reasoning` 顶层字段返回思考过程。DeepSeek R1 用 `reasoning_content`。Kimi 不用这两个字段中的任何一个——它走 `thinking: { type: enabled }` 请求体参数触发推理模式，结果放在自己的字段里。Qwen 目前不返回独立推理字段。

Peri Code 的适配层解析时同时检查 `reasoning_content` 和 `reasoning`，谁有值用谁：

```rust
// invoke.rs:195
let reasoning_text = assistant_msg["reasoning_content"]
    .as_str()
    .or_else(|| assistant_msg["reasoning"].as_str());
```

回传时统一用 `reasoning_content`——这是 OpenAI 标准字段名。所有模型至少不会因为多了一个不认识的顶层字段而报错。反过来不行，DeepSeek 不认识 `reasoning` 字段，直接反序列化失败。

content 数组里的 thinking 块是另一个雷区。DeepSeek V4 Pro 的 thinking 模式会在 content 数组里返回 `{"type": "thinking", "thinking": "..."}` 这种结构。原样塞进下一轮请求，大多数 OpenAI 兼容 API 返回 `unknown variant 'thinking'`——不认识这种类型。我们试过四家的 API，都不支持 content 里的 thinking 块，默认过滤掉。

## Kimi 的互斥参数

Kimi 的推理模式触发很简洁，请求体加 `thinking: { type: enabled }` 就行。但如果你同时设置了 `reasoning_effort`——o1/o3 系列的推理强度参数——Kimi 直接报错。

这两个字段本来就不应该同时出现。`reasoning_effort` 是 o1/o3 系列专用的，`thinking` 是另一套参数体系。但 Peri Code 的适配层是统一的，所有 OpenAI 兼容模型共用同一个结构体，字段按需组合。用户同时开启推理模式和推理强度，构造出的请求体就同时包含这两个字段。

修复用模型名匹配——检测到 Kimi 时移除 `reasoning_effort`：

```rust
// invoke.rs:403
if adapter.model.to_lowercase().contains("kimi") {
    body.as_object_mut()
        .and_then(|b| b.remove("reasoning_effort"));
}
```

## 流式里的暗坑

流式输出的分歧比非流式更隐蔽。大部分情况下四家的流式格式是一致的，推理内容的流式字段名又各走各的。

GLM 在流式里同时发 `reasoning_content` 和 `reasoning` 两份推理 delta。解析端的双字段兼容在流式端要重做一遍——同一个 `or_else` 逻辑，流式和非流式各一份。

Qwen 有另一个特殊处理。它的 API 需要客户端显式发送 `stream_options: { include_usage: true }`，才会在流式最后一个 chunk 返回 token 用量。其他 provider 不需要这个字段，加了也不报错——但 Qwen 不加就没有 usage 数据，token 统计是空的：

```rust
// invoke.rs:380
if streaming && adapter.model.to_lowercase().contains("qwen") {
    body["stream_options"] = json!({"include_usage": true});
}
```

## finish_reason 不只有三种

OpenAI 协议定义了三种标准 `finish_reason`：`stop`、`tool_calls`、`length`。实际跑起来远不止。GLM 在内容触发审核时返回 `sensitive`。各家还有 `content_filter`、`cancelled` 等变体。

Peri Code 的处理方式是枚举兜底——不认识的值全部归入 `Other`，不影响主流程：

```rust
// types.rs:129
pub fn from_openai(s: &str) -> Self {
    match s {
        "stop" => Self::EndTurn,
        "tool_calls" => Self::ToolUse,
        "length" => Self::MaxTokens,
        other => Self::Other(other.to_string()),
    }
}
```

真正麻烦的不是值不对，而是值和内容不一致。某些 provider（DeepSeek 是主要来源）返回 `finish_reason: "stop"`，但响应体里包含完整的 tool_use 块。Agent 按 `stop` 处理，把包含 tool_use 的消息当作普通回复写入状态——下一轮请求缺少配对的 tool_result，直接 400。

Peri Code 在 ReAct 适配层加了一层防御——无论 `finish_reason` 说什么，只要响应体里有 tool_use，一律按工具调用处理：

```rust
// react_adapter.rs:172
} else if response.message.has_tool_calls() {
    // 防御：某些 provider 返回 stop_reason != ToolUse
    // 但响应含 tool_use blocks，必须按工具调用处理
    tracing::warn!(
        stop_reason = ?response.stop_reason,
        tool_count = calls.len(),
        "stop_reason 与内容不一致"
    );
}
```

这行 warn 日志在 DeepSeek 上触发频率不低。

## 过滤，不是透传

最后一个分歧在消息回传上。有些模型返回 Peri Code 不认识的 content block 类型，解析为 Unknown 保留下来。回传时必须过滤掉——同时把已经通过顶层字段回传的 thinking/reasoning 块也从 content 数组里去掉。否则 content 里一份，顶层字段里一份，格式严格的 provider 直接拒绝：

```rust
// invoke.rs:68
.filter(|v| {
    let t = v["type"].as_str().unwrap_or("");
    t != "thinking" && t != "reasoning"
})
```

空 content 数组也是边界情况。过滤完如果数组为空，不能发空数组——发一个空字符串 `""`：

```rust
// invoke.rs:54
if parts.is_empty() {
    json!("")
}
```

DeepSeek 在某些情况下会在 content 数组里夹带 thinking 块，不过滤就报 400。这是线上真实踩过的坑。

## 碎，但避不开

适配四家国产模型的核心工作量——5 处运行时分支、2 处双字段兼容、1 处过滤逻辑、1 处 finish_reason 内容一致性防御。每一处都是几行代码的事，但每一处都对应一次生产环境的 400 错误。

「OpenAI 兼容」在基础对话层面是成立的，四家做得都不错。但 Agent 框架要的不是基础对话——是多轮推理、流式统计、参数互斥、消息清洗。这些能力上，每家各走各的路。模型名嗅探 + 运行时分支，是目前找到的最务实的解法。

回头看那个 GLM 的 400 错误——它不是 bug，是「兼容」这个承诺的边界。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
