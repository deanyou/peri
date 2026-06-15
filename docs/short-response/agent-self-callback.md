# 哪个编程工具能让 Agent 调用的代码回调 Agent 自己？

> 哪个编程工具能让 Agent 调用的代码回调 Agent 自己？比如 Agent 写的代码处理到中间一步需要 OCR，怎么让 Agent 在代码执行的中间步骤中断，然后回调多模态 Agent 识别文本，然后继续回到之前的代码处理（例如之后要执行 OCR 获取的文本中包含的代码）？

---

先厘清一件事——OCR 和多模态是两码事。

OCR 做的事很窄，把图片里的字抠出来。多模态模型能干的事宽得多——看图、理解布局、根据图里的内容写代码、判断下一步该干什么。题主说的「识别文本然后执行文本里包含的代码」，这已经不是纯 OCR 了，属于多模态理解。两种场景的解法不同。

## OCR——MCP 封装就够了

如果需求就是纯 OCR，只是提取图片/PDF 里的文字，不需要理解内容，那 MCP 封装是最直接的路。把 OCR 服务（比如 Tesseract、PaddleOCR）封装成 MCP server，代码里当工具调用：

```rust
mcp__ocr_server__recognize(path="/tmp/screenshot.png")
```

Peri Code 的 `SearchExtraTools` / `ExecuteExtraTool` 机制支持延迟发现这类工具，LLM 不需要一次性看到所有工具定义，用的时候再查，核心工具列表不会膨胀。

## 多模态理解——SubAgent

如果需求是看图、理解、然后基于理解的结果继续处理，那 SubAgent 更合适。把多模态模型配置成轻量模型，主流程用纯文本模型跑，遇到需要「看」的步骤时切过去：

```
代码 → 遇到图片 → Agent(subagent_type="general-purpose", model="haiku", prompt="看这张图，提取信息并继续处理")
→ 多模态 Haiku 看图、理解、执行 → 返回结果 → 主流程继续
```

SubAgent 可以独立选模型，主流程用文本模型，看图时切到多模态模型。需要把上下文写进 prompt 传过去。

子 Agent 结果通过 `ExecutorEvent` 流式回传，不打断主循环。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
