---
name: llm-log-analyzer
description: 分析 llm-gateway 代理产生的请求/响应日志。当用户说"分析日志"、"查看 LLM 请求"、"对比 session"、"检查 token 用量"、"日志里有什么"、"帮我看看 data 目录"、"哪个请求失败了"、"找一下 session 的请求"等涉及 LLM 网关日志分析的场景时使用此 skill。即使用户只是笼统地说"看看日志"或"data 里有什么"，也应触发。
---

# LLM Log Analyzer

分析 `./data/` 下的 LLM 请求/响应日志。

## 日志结构

每个请求对应一个目录，命名格式 `YYYY-MM-DD_HH-MM-SS-mmm_NNNN`：

```
data/
└── 2026-05-14_10-30-15-123_0003/
    ├── request.json      # { headers?: {...}, body?: {...} } 或裸 body
    ├── response.json      # JSON 响应（非流式）
    ├── stream.log         # SSE 流式响应原文
    └── log.txt            # 终端格式的人类可读日志
```

**request.json 格式**：较新版本为 `{ "headers": {...}, "body": {...} }`，早期版本直接是裸请求体。`headers` 中的 `x-session-id` 可按 session 追踪同一 agent 的多次请求。

## 分析工具

`scripts/llm-log-query.mjs` 提供以下子命令，用 `bun run scripts/llm-log-query.mjs <command>` 运行：

### list — 列出请求摘要

```bash
bun run scripts/llm-log-query.mjs list [--dir ./data] [--limit 20] [--model NAME] [--session ID] [--route openai|anthropic] [--after YYYY-MM-DD] [--before YYYY-MM-DD] [--errors]
```

输出表格：序号 | 请求ID | 时间 | 路由 | 模型 | Session | 消息数 | 状态 | 延迟

### show — 查看单个请求详情

```bash
bun run scripts/llm-log-query.mjs show <request-id> [--dir ./data] [--body] [--messages] [--tools] [--stream]
```

- 默认显示摘要（headers、模型、状态、延迟、token 用量）
- `--body` 显示完整请求体
- `--messages` 只显示消息列表（role + 内容前 100 字）
- `--tools` 只显示工具定义列表
- `--stream` 解析 stream.log 中的 SSE 事件

### session — 追踪一个 session 的完整请求链

```bash
bun run scripts/llm-log-query.mjs session <session-id> [--dir ./data] [--full]
```

按时间序列展示同一 session 的所有请求，显示每轮的角色和工具调用。`--full` 输出完整消息内容。

### diff — 对比两个请求的差异

```bash
bun run scripts/llm-log-query.mjs session <session-id> diff <round1> <round2> [--dir ./data]
```

对比同一 session 中第 N 轮和第 M 轮请求的 messages 差异，高亮新增/删除/修改的消息块。用于观察 agent 如何逐步构建上下文。

也可以直接对比两个请求 ID：

```bash
bun run scripts/llm-log-query.mjs diff <request-id-1> <request-id-2> [--dir ./data]
```

### stats — 统计汇总

```bash
bun run scripts/llm-log-query.mjs stats [--dir ./data] [--by model|session|route|hour]
```

输出汇总：总请求数、按维度分组（模型/session/路由/小时）的请求数、错误率。

### cache — 缓存率深度分析（重要）

```bash
bun run scripts/llm-log-query.mjs cache [--dir ./data] [--session <id>] [--by-session] [--after YYYY-MM-DD] [--before YYYY-MM-DD]
```

**这是最常用的诊断命令之一。** Prompt Cache 命中率直接影响 API 成本和延迟，每次分析日志时都应主动运行此命令，即使没有明确要求。

输出内容：

- **全局缓存率**：缓存命中 token / 总输入 token，以及缓存写入率、冷 miss 率
- **缓存健康度**：按阈值（无缓存 / < 30% / >= 30%）分级统计
- **Session 缓存趋势**（`--by-session` 或 `--session`）：逐轮展示缓存命中率变化，判断前缀是否稳定
- **自动诊断**：
    - 所有请求无缓存 → 检查 prompt caching 是否启用
    - 部分请求无缓存 → 区分冷启动（正常）vs 前缀不稳定（异常）
    - 缓存写入 > 缓存读取 → 缓存投入未回收，前缀在请求间变化
    - 逐轮下降 → messages 前缀被 prepend 打乱、tools 顺序变化、system prompt 动态段过大

#### 缓存率分析检查清单

分析日志时，务必关注以下指标：

| 指标       | 正常范围                 | 异常信号                 |
| ---------- | ------------------------ | ------------------------ |
| 缓存命中率 | > 50%（ReAct 第 2 轮起） | < 30% 持续出现           |
| 缓存写入率 | 首轮高、后续低           | 每轮都很高（前缀总在变） |
| 冷 miss 率 | 仅首轮                   | 多轮后仍有冷 miss        |
| 逐轮趋势   | 稳定或微升               | 持续下降                 |

#### 常见缓存失效原因排查顺序

1. **system prompt 变化**：用 `session <id> diff 1 2` 对比前两轮的 system 消息，检查是否有日期、cwd 等动态占位符在边界标记之前
2. **tools 数组顺序**：用 `session <id> diff 1 2` 检查 tools 列表是否一致（HashMap 迭代顺序不确定会导致序列化不稳定）
3. **消息前缀被 prepend 打乱**：用 `session <id> diff 1 2` 检查消息序列开头是否被插入了新消息
4. **cache_control 断点丢失**：用 `show <id> --body` 检查请求体中 system/tools/messages 的 cache_control 标记是否存在

## 分析流程

面对用户的日志分析需求，按以下步骤工作：

1. **定位范围**：先用 `list` 了解日志总量和时间范围，必要时用 `--after`/`--before` 缩小范围
2. **缓存率检查**（每次分析都应执行）：运行 `cache` 命令，检查缓存命中率和健康度。如果发现问题，用 `--by-session` 查看逐轮趋势，再用 `session <id> diff` 定位前缀不稳定的具体原因
3. **按需下钻**：
    - 查某个 session 的完整交互 → `session`
    - 查某个具体请求 → `show`
    - 对比两轮请求差异 → `session ... diff` 或 `diff`
    - 统计全局概况 → `stats`
4. **解读结果**：将工具输出翻译为用户能理解的结论（如"这个 session 共 5 轮 LLM 调用，缓存命中率 72%，第 3 轮因 tools 列表变化导致缓存失效"）

## 注意事项

- 工具路径相对于 skill 目录：`scripts/llm-log-query.mjs`
- 数据目录默认为 `side-projects/llm-gateway/data/`，如果用户指定了其他目录用 `--dir` 覆盖
- `request.json` 的 headers 字段中 `authorization`、`api-key` 等敏感字段已被脱敏（只保留前 12 字符 + `…`），分析时注意不要试图还原
- stream.log 是原始 SSE 文本，内容可能很大，展示时注意截断
