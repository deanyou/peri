# Peri Code 的 ACP 接入——让 Zed、JetBrains、OpenClaw 都能驱动同一个 Agent

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

在 Zed 的 `settings.json` 里加几行：

```json
"agent_servers": {
  "peri": {
    "type": "custom",
    "command": "peri",
    "args": ["acp"]
  }
}
```

Zed 的 Agent 面板就能驱动 Peri 的完整能力。换到 JetBrains 系 IDE，同样的配置格式，同样的效果。用 OpenClaw 批量跑任务，Peri 也在那里——不是适配版，是同一个核心。

这件事成立，是因为 Peri 实现了 ACP（Agent Client Protocol）。ACP 对 AI Agent 的作用，类似 LSP 对语言服务器——标准化 IDE 和 Agent 之间的通信，任何实现了协议客户端的工具，都能接入任何实现了协议服务端的 Agent。Peri 在协议服务端这一侧。

## 不实现 ACP 就要为每个 IDE 单独适配

在接入 ACP 之前，假设要让 JetBrains 用上 Peri，选择只有两个——要么 JetBrains 方实现一套专用 API，要么 Peri 方针对 JetBrains 写一套适配层。两条路都死：IDE 厂商没理由为一个第三方 Agent 定制协议，Peri 没资源为每个 IDE 各写一套。

ACP 是已经跑通的第三条路。Zed 实现了 ACP 客户端，JetBrains 实现了 ACP 客户端，OpenClaw 基于 ACP 构建，acpx 工具链消费 ACP Agent——这些工具不关心 Agent 内部是 Rust 还是 Python，不关心 ReAct 循环怎么跑，只关心 Agent 是否响应 `initialize`、`session/new`、`session/prompt`。Peri 响应这三个，就进了整个生态。

## 同一个核心，三条进入路径

ACP 接入之后，Peri 的三条运行路径——TUI 交互、`-p` 非交互、IDE stdio——共享同一个执行核心。

TUI 路径通过内存 channel（`MpscTransport`）连接 ACP 层，TUI 本质上是 Peri 自己的 ACP 客户端。stdio 路径接 IDE——Zed 启动 Peri 进程，通过 stdin/stdout 的 JSON-RPC 2.0 通信，和 LSP 的工作方式一模一样。无头模式在这条路径上扩展——Agent 作为后台服务运行，前端换成 Web UI 或 VS Code 插件，协议层是唯一的路径。

三条路径里，Agent 的代码一行没变。

## Agent 不需要知道对端是谁

**事件流向外，控制指令向内。** IDE 通过 `session/new`、`session/prompt`、`session/cancel` 向 Agent 发控制指令，Agent 通过 `session/update` 通知流推状态变更——文本流、工具调用进度、推理内容。双向都有明确定义，没有协议外的私有通道。

**权限审批变成协议 RPC。** Peri 的 HITL（Human-In-The-Loop）机制在 Agent 需要执行危险操作时发起审批——原本是 TUI 弹窗。接入 ACP 后，这个动作变成 `session/request_permission` RPC：Agent 发起请求，描述要执行的工具调用和可选响应（allow_once / allow_always / reject_once / reject_always），由客户端决定怎么处理。Zed 弹 Zed 的审批 UI，OpenClaw 走自己的自动化策略，`-p` 模式无条件批准——Agent 的代码不区分对端是谁。

事件映射只维护一份。Peri 内部的 `ExecutorEvent` 到 ACP `SessionUpdate` 的转换规则在协议层里，不分叉。早期 TUI 和 stdio 路径分别做了一套映射，一次事件语义的变更要改两个地方，很快就漂移了。合并成一份之后，新增事件类型只改协议层，两条路径自动同步。

## 事件映射漏更新，IDE 端状态就坏了

事件映射有维护负担。新增 `ExecutorEvent` 变体必须同步更新 ACP 映射层，否则事件被静默丢弃，IDE 端状态不一致。这个坑已经踩过不止一次，专门在 CLAUDE.md 里记了陷阱警告。

协议语义不能随意变更。`session/update` 的 10 种 `SessionUpdate` 变体一旦被 Zed、JetBrains、OpenClaw 依赖，改语义就是破坏性变更。加字段没问题，改含义要慎之又慎——这是任何公开协议的固有成本，不是 Peri 独有的。

调试链路变长了。一次工具调用从 Agent 内部到 IDE 端，经过事件生成 → 映射 → 序列化 → stdio → IDE 渲染，中间任何一环出问题，需要在协议两端分别排查，不能直接在 Agent 方法上看 IDE 的调用栈。

Peri 现在是 ACP 生态里的一个合法 Agent。配置几行，Zed 或 JetBrains 就能驱动它——后面的事，用户不需要关心。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
