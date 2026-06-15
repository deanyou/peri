# Peri Agent 编辑工具设计复盘：行号和 Diff 为什么不如字符串替换

我们造 LineEdit 的出发点不是要替代 Edit——是想给 Agent 造一个更好用的编辑工具。

Edit 工具靠字符串精确匹配，我们遇到过几次它匹配失败的情况，模型改了半天什么都没写进去。于是我们觉得，行号定位更稳定，行就在那里，1 到 N，不会有歧义。

这个判断错了。不是错在实现，是错在对 AI 能力的理解上。花了三个版本才想清楚。

## 字符串匹配，反而是对的

Edit 工具有一个让人不安的地方——它靠字符串精确匹配。`old_string` 和文件里差一个空格就失败。这看起来脆。

但我们忽略了一件事——模型生成 `old_string` 的时候，那段代码就在它的 context 里，它刚读过，它要做的只是原样复述。复述自己刚看过的内容，是 LLM 最擅长的事。

行号不一样。模型从文件里读到「第 58 行是 content_to_openai 的入口」，然后继续往下读了几百个 token，处理了工具返回值、分析了调用链，最后生成了「替换第 58 到 72 行」的请求——它记住的是「大概在那里」，不是精确的 58。我们第一次用真实任务测试，模型要改 `peri-agent/src/llm/openai/invoke.rs` 第 58 行，实际代码在第 63 行，偏了 5 行，工具返回成功但什么都没改，模型还以为完成了。

这不是偶发，是 LLM 处理位置信息的结构性限制。字符串是内容，模型擅长复现；行号是元数据，模型在生成响应时已经离原始位置几千个 token 远了，偏移是必然的。

我们拿错误的问题（字符串匹配不精确）推出了错误的方案（用行号替代），然后造了三个版本来修一个根本不存在的问题。

## 三个版本，三次加码，每次都加错了地方

**V1——行号偏移**

V1 用行号范围定位。模型读完文件，过了几百个 token 的工具返回、调用链分析，生成了这样的请求：

```json
{
  "file": "peri-agent/src/llm/openai/invoke.rs",
  "start_line": 58,
  "end_line": 72,
  "new_content": "..."
}
```

实际上那段代码在第 63 到 77 行，偏了 5 行。工具执行，什么都没改到，返回了成功。模型还以为改完了，继续往下跑，下一轮调用直接崩。

这不是偶发。模型记住的是「大概在那里」，不是精确行号。离读文件的那一刻越远，偏得越多。

**V2——两个弱约束叠在一起**

V2 加了 `expected_lines` 验证，让模型附上它预期看到的内容：

```json
{
  "file": "peri-agent/src/llm/openai/invoke.rs",
  "start_line": 58,
  "end_line": 72,
  "expected_lines": [
    "pub(super) fn content_to_openai(",
    "    content: &MessageContent,",
    "    supports_thinking_content: bool,"
  ],
  "new_content": "..."
}
```

行号还是偏的，`expected_lines` 里的内容也对不上实际的第 58 行。两个约束同时错，验证直接失败，操作中止。比 V1 更脆——V1 至少还会静默跳过，V2 直接报错停下来。

**V3——模型不擅长生成 diff**

V3 换成 unified diff 格式，看起来最规范：

```diff
--- a/peri-agent/src/llm/openai/invoke.rs
+++ b/peri-agent/src/llm/openai/invoke.rs
@@ -58,15 +58,20 @@
 pub(super) fn content_to_openai(
     content: &MessageContent,
-    supports_thinking_content: bool,
+    supports_thinking_content: bool,
+    adapter: &ChatOpenAI,
 ) -> Value {
```

问题是模型经常搞错 `@@` 里的行号、漏掉上下文行、`-`/`+` 前缀错位。这套格式是 diff 命令输出给人看的，不是让语言模型生成的。5 级 fallback 匹配引擎、3 层验证、tree-sitter AST——我们加了这么多容错层，根本上是在给一个不适合模型的格式打补丁。

最后 tree-sitter 层因为验证成本太高先删掉，然后整个 LineEdit 被一行提交 `feat: remove LineEdit beta feature` 清理干净。

## 为 AI 设计工具，直觉是反的

Edit 工具的「粗糙」——字符串精确匹配——在人类工具设计里确实是缺点。但放到 AI 这里，它恰好把任务转化成了 LLM 最擅长的操作，复述刚读过的内容。

行号和 diff 格式把任务转化成了位置记忆和格式生成，这两件事 LLM 做起来都比字符串复现差得多。我们每次加新功能，本质上都是在把「字符串匹配」这个 LLM 友好的约束，换成「位置信息维护」或「格式协议生成」这类 LLM 不友好的约束。工具看起来越来越专业，实际上越来越难用。

这件事的核心是——**为 AI 设计工具，要顺着它的生成方式，而不是顺着人类的认知习惯**。人类觉得行号精确、diff 规范，这些直觉在 AI 工具设计里是反的。

我们花了三个版本、几天时间，把这个道理从头验证了一遍。

---

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
