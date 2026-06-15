# Peri Code 内存优化：从 Claude Code 的 2GB 到长任务稳定 100MB

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 编写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

Claude Code 在经历几十轮对话后内存会膨胀到 2GB，随后 OOM。根本原因在于 Bun 的流式输出在处理 LLM 流式响应时会产生严重的内存碎片——已释放的内存页因为相邻的存活对象将其"钉住"而无法归还给操作系统，碎片就这样无限积累。我们在维护 Claude Code Best 期间花了大量时间追查和修复这个问题，最终确认这是 Bun runtime 的结构性限制，JS 生态很难绕过。

这也是我们决定用 Rust 从头重写，做出 Peri Code 的原因之一。全新的 Peri Code 会话启动时约 40MB，长任务运行期间稳定在 100MB 左右，不再增长。本文记录我们是如何做到这一点的。

## Peri Code 自身也走过弯路

Peri Code 用 Rust 编写，没有 Bun runtime 的历史包袱，但内存问题并没有自动消失。早期版本每轮对话 RSS 增长 40MB，`/clear` 之后也不回落。

第一直觉是内存泄漏。但堆快照给出了不同的答案：`allocated` 并没有在增长——对话前 9.5MB，五次工具调用后 9.0MB，反而略有下降。每轮会产生 70 万次 malloc 调用，其中 97.3% 在 prompt 结束前就已释放。严格意义上并不存在泄漏，问题出在别处。

## 根本原因：allocator 碎片 + runtime 基础设施

RSS 增长可以拆成两块：

```
RSS ~200MB
├── allocator 碎片（~70MB）     ← 已释放但内存页无法归还给 OS
└── 非 allocator runtime（~120MB）  ← tokio 线程栈 + reqwest 连接池 + TLS 缓冲区
```

存活对象只有 9MB，其余全是上述两类。

## 分配风暴：四个来源

### 风暴 A：最严重的问题

`SubAgentMiddleware` 的 `before_agent` 钩子在每次 ReAct **迭代**时都对所有消息做完整克隆：

```rust
*pm.write() = state.messages().to_vec();  // 完整深拷贝
```

单次对话可能有 10～50 次 ReAct 迭代。当消息历史增长到 500 条时，每次克隆就是 1～2MB。乘以迭代次数，一轮对话会产生 50～100MB 的临时分配。这些大块内存横跨多个内存页；释放后，相邻的存活对象（MCP Pool、ToolSearchIndex 等）将这些页"钉住"，无法归还给操作系统。

修复方案：延迟克隆——只在真正调用 SubAgent 工具时才执行。

### 风暴 B：每轮克隆完整消息历史

```rust
state.history.clone()  // 每轮 1～2MB
```

改用 `std::mem::take` 直接转移所有权——零拷贝。

### 风暴 C：`prompt_locks` HashMap 泄漏

每次 `session/prompt` 都会创建一条锁记录，但 `session/close` 从未清理。每次 `/clear` 都会多出一条过期条目。单条记录体积很小，但这是逻辑错误——已修复。

### 风暴 D：`build_agent` 每轮重建大对象

每次 `session/prompt` 调用都会执行 `build_agent()`，创建完整的 ReActAgent + 16 个中间件 + 一个 LLM 实例（其中 `reqwest::Client` 每个 1～2MB）。对象在 prompt 结束后会被正确释放，但释放过程本身就会触发 68 万次瞬态 malloc/free 调用，导致 jemalloc 的 arena slab 碎片化，内存无法归还给操作系统。

解决方案是 `AgentPool`：在 session 级别缓存 LLM 实例。第一次 prompt 执行完整构建，后续 prompt 通过 provider 指纹（`"provider_name:model_name"`）检查缓存，命中时复用已有的 `reqwest::Client` 和 TLS session。切换模型时指纹不匹配，自动触发重建。

## 换 allocator 没用

我们尝试过调优 jemalloc（`dirty_decay_ms=200`、`lg_tcache_max=16`）——效果有限。切换到 mimalloc（`DECOMMIT`、`BACKGROUND_THREAD`、`PAGE_RESET`）并在 `/clear` 时调用 `mi_collect(true)`，也没有改变 RSS 的增长模式。

结论：这个问题无法在 allocator 层面修复。根源是分配风暴本身——正确的方向是同时减少分配次数和单次分配的大小。

## 结果

修复风暴 A/B/C/D 之后，在长任务场景下连续数百轮对话，RSS 稳定在 100MB 左右，不再线性增长。相比 Claude Code 的 2GB，差距达到一个数量级。在 Milk-V Jupiter（搭载 8GB RAM 的 RISC-V 开发板）上，稳定在 70MB 不再变化。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
