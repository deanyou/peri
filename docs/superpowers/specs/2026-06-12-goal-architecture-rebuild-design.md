# Goal 子系统架构 Rebuild 设计

**日期**: 2026-06-12
**状态**: Draft（设计完成，待实施）
**关联 Issue**: `spec/issues/2026-06-12-goal-set-error-yet-success-and-immediate-compact-and-budget-stale.md`
**分支**: `feat/goal-rebuild`（从 `main` 干净起步，旧实现保留在 `feat/goal-steering` 作参考）

## 概述

对复刻自 codex `/goal` 的 Goal Steering 子系统进行架构级返工。初始 review 识别出 8 个问题（P1-P8），经 17 轮 grill 深度访谈后重新审视——发现部分诊断过时（已在 major #1/#2 修复）、部分提议技术上不可行（middleware trait 不支持接管 ReAct 循环、prepend System 破坏 cache）。Rebuild 聚焦真正的痛点：**用户消息在 continuation 期间无法进入 agent**、**steering 每轮注入导致 context 膨胀**、**计费缺少 fallback**、**可观测性不足**。

## 需求

- **契约关系明确化**：goal 作为「用户对智能体的任务委托」，agent 在 goal 范围内自主决策，用户仅 pause/resume/clear/补充指令干预
- **用户消息可打断 continuation**：机制 3——消息进入 `pending_user_message`，下一轮作为 content 注入，continuation 不重启、goal 仍 Active
- **steering 注入优化**：从「每轮注入」改为「事件性 + 工具查询 + 兜底」Y 模型，事件包括 set/updated/budget_limit/compact 后/百分比阈值
- **计费 hybrid fallback**：优先用 LLM 真实 usage，缺失时用 `char/4` 估算并标记 `estimated=true`，TUI 显示 `≈`
- **可观测性**：TUI 形态 A（对话流内联）+ B（GoalPanel 历史）+ C（status_bar 简化版）+ Langfuse trace
- **保持并发安全**：保留 RwLock + Semaphore + read-and-reset + epoch 模型，新增字段复用 `parking_lot::RwLock` 短锁
- **保留优于清除**：error/cancel 时不主动清除 goal/history/pending，给用户反悔空间
- **prompt cache 前缀稳定**：所有 steering 注入走 `add_message(Human, <system-reminder>)` 尾部追加，绝不破坏 `frozen_system_prompt`

## 架构

### 设计原则

| 原则 | 说明 |
|------|------|
| **任务委托语义** | goal 是用户对智能体的委托，agent 在 goal 范围内自主决策，用户通过 pause/resume/clear/补充指令干预 |
| **用户消息优先** | continuation 期间用户消息可打断（机制 3），作为下一轮 content，continuation 不重启 |
| **事件性注入** | steering 不每轮注入，仅在事件（set/updated/budget_limit/compact 后/百分比阈值）触发 |
| **前缀缓存稳定** | 注入走 `add_message` 尾部追加，接受 messages 膨胀代价，不破坏 `frozen_system_prompt` |
| **保留优于清除** | error/cancel 时保留 goal Active / history / pending，让用户有反悔空间 |
| **内存优于 store** | store 写入失败时退化为纯内存模式（snapshot 读仍可用），不阻塞 agent |
| **工具优于状态** | SubAgent 是工具，error 通过 tool_result 返回，不自动影响 goal 状态 |
| **可观测性** | goal 状态变化、steering 注入、continuation 轮次、计费增量都通过 Langfuse trace + TUI 可见 |

### 数据流

**continuation 主循环**（保留在 ACP `session/prompt` handler 层，机制 3 优化）：

```
用户消息到达（goal Active 期间）
  → GoalState.pending_user_message = Some(msg)  ← 不启动新 prompt handler
当前 in-flight LLM 调用跑完
  → execute_prompt 返回
continuation loop 下一轮:
  content = take_pending_user_message().unwrap_or_default()
  execute_prompt(content, history)
continuation 继续（goal 仍 Active，不重启）
```

**steering 注入数据流**（Y 模型，事件性）：

```
事件触发（set/updated/budget_limit/compact 后/百分比阈值）
  → GoalMiddleware::before_model 读取触发标志
  → 按事件类型选择模板渲染（~50/200/500 tokens）
  → add_message(Human, <system-reminder>) 尾部追加  ← 不破坏 cache 前缀
  → 设置 context 去重标志 `goal_steering_injected`
  → emit `peri/goal_steering_injected` 通知到 TUI
```

**计费数据流**（hybrid fallback）：

```
after_model 钩子
  → 优先读 reasoning.usage
  → 缺失时 char_count/4 估算（input + output）+ estimated=true
  → delta = input - cache_read + output
  → 累加到 GoalState.accounting.pending_token_delta
after_agent 或 on_error
  → account_progress: store 单事务原子累加 + 内存 snapshot 同步
  → 计算当前 usage_pct，检查百分比阈值跨越
```

### 组件变更

#### 1. 概念层：契约模型 A + 两条契约条款

**契约模型 A（任务委托）**：

```
用户：设定 goal → 把"完成权"交给智能体
智能体：在 goal 范围内自主决策、连续执行（continuation）
用户角色：仅 pause / resume / clear / 补充指令（机制 3 打断）
终态判定：智能体自评（update_goal 工具）或 budget 耗尽
```

**契约条款 1（用户消息角色）**：用户在 goal Active 期间发送的消息是**补充指令**，不是新任务。消息打断 continuation（机制 3），作为下一轮 content。

**契约条款 2（终态判定权）**：终态判定为 **agent 单方宣告 + TUI 强提示 + 低摩擦反悔**（模型 α）。agent 调 `update_goal(Complete|Blocked)` 直接翻终态，TUI 强提示，用户可通过 `/goal resume` 反悔。

#### 2. 用户消息打断机制（机制 3）

**关键参数**：
- in-flight LLM 调用**不取消**（让 LLM 跑完，避免 cancel 风暴）
- 多条消息**覆盖**（`Option<String>`，只保留最后一条）
- messages 数组内顺序：**steering 在前，用户消息在后**（LLM 最后看到用户消息）
- 用户消息**不额外触发** steering 注入（百分比规则独立运作）

**并发保护**：复用 GoalState 的 `parking_lot::RwLock`（短锁，无 await，天然互斥）。

**清理时机**：

| 状态变化 | pending_user_message |
|---------|---------------------|
| set_goal（新 goal） | 保留 |
| clear | 清零 |
| set_status(Complete/Blocked) | 清零 |
| set_status(Paused) | 保留 |
| set_status(Active)（resume） | 保留 |

#### 3. GoalState 字段扩展

```rust
struct GoalStateInner {
    goal: Option<ThreadGoal>,
    accounting: GoalAccountingState,
    objective_just_updated: bool,
    should_continue: ShouldContinueFlag,
    store: Arc<dyn GoalStoreTrait>,
    thread_id: String,
    // 新增字段
    pending_user_message: Option<String>,       // 机制 3
    last_injected_usage_pct: Option<f32>,       // 百分比注入去重
    last_injected_wall_clock: Option<Instant>,  // budget=None 时间周期
}
```

#### 4. 注入策略：Y 模型

| 触发条件 | 注入内容 | 频率 |
|---------|---------|------|
| T1: set / objective_updated | 完整模板（~500 tokens） | 事件性 |
| T2: budget_limit（翻 BudgetLimited） | budget_limit 模板 | 事件性 |
| T4: compact 后 | compact_reorient 模板 | 事件性 |
| T5: budget 阈值（80% warning / 95% urgent） | 警告模板（~200 tokens） | 事件性 |
| T6: 百分比步长（每 10%） | 轻量一行（~50 tokens） | 周期性 |

**百分比注入机制**：
- 每次 `account_progress` flush 后，比较当前 usage% 与上次注入时的 usage%
- 跨越 10% 边界 → 注入轻量 reminder
- 跨越 80% 边界 → 升级为 budget_warning
- 跨越 95% 边界 → 升级为 budget_urgent

**budget=None（无上限）时**：百分比注入失效，回退到时间周期（每 5 分钟 wall_clock 注入一次）。

**实现路径**：`add_message(Human, <system-reminder>)` 尾部追加（接受 messages 膨胀，不破坏 prompt cache 前缀）。

**用户消息打断时不额外注入**：当 continuation 被 `pending_user_message` 打断（机制 3）时，下一轮的 steering 注入仍按百分比规则独立判定——不因用户消息到达而额外触发 T1-T5 事件性注入。Ctrl+C 打断的场景见 §组件变更 12 error/cancel 路径处理表（continuation 停止，不进入下一轮）。

#### 5. 计费与 fallback

**hybrid 策略**：
- 优先用真实 LLM usage（`after_model` 钩子提取 `reasoning.usage`）
- usage 缺失时用 `char_count/4` 估算（input + output 都算）
- 估算值标记 `estimated=true`，TUI 显示 `≈` 符号（如 `≈12K/200K`）

**估算值处理**：直接累加（无宽容判定）。`include_usage` 修复后 usage 缺失是边缘情况，估算值占比极低。

**计费公式**（不变）：`delta = input_tokens - cache_read_input_tokens + output_tokens`

#### 6. compact 与 goal 交互

| 决策点 | 选择 |
|--------|------|
| compact 后注入检测 | CompactMiddleware 设置 context 标志 `compact_just_happened`，GoalMiddleware before_model 读取并清零 |
| 注入模板 | 专用 `compact_reorient` 模板（强调"记忆刚被压缩，重新对齐目标"） |
| compact LLM 计费 | 计入 goal budget（compact 是 agent 活动的一部分） |
| steering 压缩 | 作为普通消息被压缩（Y 模型下注入频率低，影响小） |
| 摘要内容 | 摘要模板加入 goal 提示（`当前 goal: <objective 80 chars>, 已用 N/200K`） |
| compact 后 continuation | 继续（compact 是 ReAct 内部操作，对 continuation 透明） |

#### 7. 持久化与 resume

**持久化边界**：

| 状态 | 持久化？ | 理由 |
|------|---------|------|
| ThreadGoal（objective/status/budget/usage/goal_id/timestamps） | ✓ | 事实数据，必须跨 session |
| pending_user_message | ✗ | 即时通道，断开即丢弃 |
| pending_token_delta | ✗ | 增量缓冲，断开可接受丢失 |
| pending_time_delta_seconds | ✗ | 同上 |
| last_injected_usage_pct | ✗ | 重新计算即可 |
| injection_history | ✗ | 过程记录，用 Langfuse trace 替代 |
| continuation_guard.rounds | ✗ | resume 重置 |
| should_continue | ✗ | resume 设 false |
| objective_just_updated | ✗ | resume 后按规则注入 |

**resume 策略**：保守恢复
- resume 后 goal 保留（status/objective/budget/usage 都在），但 continuation **不自动启动**
- 用户需发一条消息触发新 prompt handler
- `should_continue` 设为 false（continuation 不启动）

**time_used_seconds 口径**：只算 agent 活动时间
- `wall_clock_baseline` 在 `begin_turn` 重置
- session 断开期间不计入
- resume 后 `hydrate` 重置 baseline

#### 8. TUI 可观测性（形态 A + B + C）

**形态 A（对话流内联）**：
- 一行轻量标记 + 可点击展开
- 按 reason 着色：set/updated（蓝）/ periodic（灰）/ budget_warning（黄）/ budget_urgent（红）/ compact_reorient（紫）
- 注入是独立事件（不关联到 BaseMessage），按 timestamp 与 messages 交错显示

**形态 B（GoalPanel 历史）**：
- 时间线倒序（最近在上）
- 每条可展开看完整 content
- 顶部显示当前 goal 状态，底部显示统计

**形态 C（status_bar 简化版）**：
- 格式：`◎ goal: <objective 80 chars> | <usage> | <time>`
- 无 goal 时：`○ no goal (/goal set to create)`
- budget_warning（80%）时变黄，budget_urgent（95%）时变红 + 闪烁

**渲染协议**：
- 新增 `peri/goal_steering_injected` 通知（与 `peri/goal_update` 平行）
- 完整 payload（含 round/reason/usage_pct/tokens_used/token_budget/content/timestamp）
- TUI 维护 `injection_events: Vec<GoalSteeringInjection>` + `expanded_injections: HashSet<InjectionId>`

#### 9. 终态判定

**模型 α（agent 单方宣告 + TUI 强提示 + 低摩擦反悔）**：

| 触发 | continuation | goal status | TUI 提示 |
|------|-------------|-------------|---------|
| agent update_goal(Complete) | 停止 | Complete | status_bar 高亮 + GoalPanel 闪烁 |
| agent update_goal(Blocked) | 停止 | Blocked（**必须附带 reason**） | 显示 reason + 建议干预 |
| budget 耗尽 | 停止 | BudgetLimited | status_bar 红色 |
| 用户 `/goal resume` | 重启（rounds 重置） | Active | 恢复正常 |
| 用户 `/goal complete`（主动） | 停止 | Complete | 同 agent 触发 |

#### 10. 命令体系扩展

**子命令**：

| 子命令 | 行为 | 新增？ |
|--------|------|--------|
| `set <obj> [--budget N\|none]` | UPSERT goal（新 goal_id） | 已有 |
| `edit <obj>` | 仅改 objective（保留 goal_id，触发 objective_updated steering） | **新增** |
| `budget <N\|none>` | 仅改 budget（保留其他字段） | **新增** |
| `pause` / `resume` / `clear` / `show` | 已有 | 已有 |

**GoalPanel 快捷键**（混合模式）：

| 键 | 行为 |
|----|------|
| ↑↓ | 导航注入历史 |
| Enter | 展开/折叠注入详情 |
| `s` | set goal（编辑模式） |
| `e` | edit objective（编辑模式） |
| `b` | edit budget（编辑模式） |
| `p` / `r` | pause / resume |
| `c` | clear（带确认） |
| Esc | 关闭 panel / 取消编辑 |

**与其他 Immediate 命令的边界**：
- `/rewind`：**不影响 goal**（goal 是独立持久状态，rewind 是消息级回滚）
- `/compact`：compact 后必注入 + 摘要含 goal 提示
- `/clear`（新 session）：新 session 无 goal（goal 是 session 级）

#### 11. SubAgent 隔离

| SubAgent 类型 | goal 感知 | 计费 | steering 注入 |
|--------------|----------|------|--------------|
| 同步（fork foreground） | 只读（可查 get_goal） | 计入父 budget | 启动时一次性注入 |
| Background（fork background） | 完全隔离 | 不计入 | 不注入 |
| Normal（无 fork） | 同步 | 计入 | 启动时一次性注入 |

**实现**：
- 同步/Normal SubAgent：builder 注入 `goal_state=Some(parent_goal_state.clone())`，挂轻量 `GoalAccountingMiddleware`（仅 `after_model` → `record_token_usage`），工具集仅暴露 `get_goal`
- Background Agent：builder 注入 `goal_state=None`，不挂任何 goal middleware，不暴露 goal 工具
- SubAgent 完成后的结果**不直接影响**父 goal（父 agent 自行判断）

#### 12. error/cancel 语义

**error/cancel 路径处理表**：

| 触发 | continuation | goal status | history | pending_user_msg | 计费 |
|------|-------------|-------------|---------|------------------|------|
| LLM error | 停止（errored） | Active（保留） | 保留 | 保留 | best-effort flush |
| Ctrl+C cancel | 停止（cancel） | Active（保留） | 保留（TRAP 7） | 保留 | best-effort flush |
| SubAgent error | 继续（父 agent 处理） | Active | 保留 | 保留 | SubAgent tokens 已计 |
| store 写入失败 | 继续 | 内存镜像更新 | 保留 | 保留 | 内存 pending 累积 |
| goal 自动 BudgetLimited | 停止 | BudgetLimited | 保留 | 清零 | 已 flush |
| agent update_goal(Complete) | 停止 | Complete | 保留 | 清零 | 已 flush |
| agent update_goal(Blocked) | 停止 | Blocked | 保留 | 清零 | 已 flush |

**三大原则**：保留优于清除 / 内存优于 store / 工具优于状态。

**`reconcile_already_done` 机制**：完全保留（issue 2026-05-25 已验证有效，rebuild 不改 TUI 层）。

#### 13. Langfuse trace 集成

| 观测点 | 记录方式 |
|--------|---------|
| goal 状态变化 | event（set/pause/resume/clear/Complete/Blocked/BudgetLimited） |
| steering 注入 | event（含 round/reason/usage_pct/content） |
| continuation 轮次 | 每轮 span（`continuation_round_N`） |
| 计费增量 | event（token_delta/time_delta/usage_pct/estimated） |

**span 结构**：goal events 附加到现有 LLM trace（不创建独立 span）。

**与 GoalPanel 的关系**：GoalPanel 的 `injection_history` 是 session 级内存状态（不持久化），Langfuse trace 是跨 session 持久记录。两者互补——GoalPanel 实时调试，Langfuse 长期审计。

### 边界情况

| 场景 | 处理 |
|------|------|
| continuation 中多条用户消息到达 | 覆盖（只保留最后一条） |
| 用户消息与百分比注入同时触发 | 两者独立运作，messages 中 steering 在前、用户消息在后 |
| budget=None 且长时间运行 | 回退到 5 分钟 wall_clock 周期注入 |
| compact 与 steering 注入同轮发生 | compact_reorient 优先注入；百分比注入跳过本轮 |
| SubAgent error | 不影响父 goal status，error 通过 tool_result 回到父 agent |
| store 写入失败 | 内存镜像仍可读，pending 累积到内存；下轮重试时 best-effort flush |
| resume 后未发消息触发 prompt | continuation 不启动，goal 保留 Active；用户首条消息触发新 prompt handler |
| `/goal edit` 触发 `objective_updated` steering | 标志 `objective_just_updated=true`，下次 before_model 按规则注入 |
| GoalPanel 注入历史无限增长 | 滚动窗口上限（默认最近 100 条） |
| 预算将耗尽但未触发 80% 阈值 | 百分比机制保证每 10% 一次 reminder，不会盲冲到 100% |

### 与原 review 文档的差异

| review 提议 | Rebuild 决策 | 差异说明 |
|------------|-------------|---------|
| continuation 移到 middleware 层 | **保留在 prompt handler 层**（机制 3 优化） | middleware trait 不支持接管 ReAct 循环；改为优化用户消息交互 |
| steering 用 prepend_message(System) | **保留 add_message(Human)** | prepend 破坏 cache_control 标记位置；接受 messages 膨胀代价 |
| 计费 fallback char/4 | **hybrid + estimated 标记** | 采纳并增强（TUI 显示 ≈） |
| 双重命令注册（P4） | 已修复（TUI 透传） | review 诊断过时 |
| peri/goal_update 穿透（P5） | 已修复（major #2 统一推送） | review 诊断过时 |
| GoalState 跨 session 持久化（P6） | 保守恢复（不自动续跑） | 改进 |
| SubAgent 隔离依赖传参（P7） | 只读感知 + 同步计费 | 增强 |
| account_progress 不推送（P8） | 百分比注入 + warning/urgent 阈值 | 改进 |

**新增设计（review 未覆盖）**：
- 契约模型 A + 两条契约条款（概念层）
- 机制 3（用户消息打断 continuation）
- Y 模型（事件性注入策略）
- TUI 形态 A + B + C（可观测性）
- compact 与 goal 交互（compact_reorient + 摘要含 goal）
- error/cancel 三原则
- Langfuse trace 集成
- 命令体系扩展（`/goal edit` / `/goal budget`）

## 不做什么

- **不把 continuation 移到 middleware 层**：middleware trait 没有「ReAct 迭代结束后、下一轮开始前」的钩子，强行接管 ReAct 循环会破坏 trait 语义
- **不用 prepend_message(System) 注入 steering**：会破坏 `cache_control` 标记位置，接受 messages 膨胀代价优于 cache 失效
- **不取消 in-flight LLM 调用接收用户消息**：让 LLM 跑完，避免 cancel 风暴和资源浪费
- **不持久化 `pending_user_message` / `injection_history`**：前者是即时通道，后者用 Langfuse trace 替代
- **不在 resume 时自动启动 continuation**：保守恢复，让用户显式触发新 prompt
- **不让 SubAgent 直接影响父 goal status**：SubAgent 是工具，结果通过 tool_result 让父 agent 自行判断
- **不做多候选 prediction 切换 / 不做 steering A/B 测试**：超出本次 rebuild 范围
- **不调整 reconcile_already_done 机制**：issue 2026-05-25 已验证有效，TUI 层不动
- **不引入新的 middleware trait 钩子**：用 context 标志 + before_model 协调（如 `compact_just_happened`）
- **不调整 frozen_system_prompt 生成机制**：steering 注入完全在 frozen 边界之外

## 实施阶段

| Phase | 内容 | 依赖 | 核心改动 |
|-------|------|------|---------|
| **1. 核心机制** | 机制 3 + Y 模型注入 + compact_reorient | 无 | GoalState 新增 pending_user_message；GoalMiddleware 改为事件性注入；CompactMiddleware 设置 context 标志 |
| **2. 计费与 fallback** | hybrid fallback + 百分比参数 + budget=None 时间周期 | Phase 1 | after_model 增加 char/4 fallback；新增 last_injected_usage_pct 字段 |
| **3. TUI 与命令** | 形态 A+B+C + peri/goal_steering_injected + /goal edit/budget | Phase 1 | TUI 新增 injection_events 渲染；GoalPanel 历史列表；ACP 新通知协议 |
| **4. SubAgent 隔离** | 只读感知 + 同步计费 middleware + Background 独立 | Phase 1 | SubAgent builder 分流；新增轻量 GoalAccountingMiddleware |
| **5. 持久化与 resume** | 保守恢复 + time 口径调整 | Phase 1 | hydrate 不设 should_continue；wall_clock_baseline 重置逻辑 |
| **6. Langfuse trace** | goal events + continuation span + billing event | Phase 1-2 | LangfuseTracer 扩展 trace_goal_event 方法 |
| **7. error/cancel 验证** | 确认 issue 2026-05-25/26/29 在新架构下仍有效 | Phase 1-6 | 集成测试覆盖所有 error 路径 |

## 开放问题

1. **百分比注入的检测时机**：account_progress flush 后立即检查？还是 before_model 时检查？
2. **compact_reorient 模板的具体内容**：需要起草并测试（强调"记忆刚被压缩"的语气）
3. **GoalPanel 注入历史的内存上限**：长 session 可能累积大量注入，需要滚动窗口或上限（如最近 100 条）
4. **hybrid fallback 的 char/4 系数**：是否需要按 provider 调整（CJK 内容 char/4 可能偏低）？
5. **同步 SubAgent 计费的 middleware 注册**：如何避免与父 GoalMiddleware 冲突（middleware 链顺序）？
6. **`/goal edit` 与 `objective_updated` steering 的交互**：edit 后是否立即注入，还是等下次 before_model 按规则注入？

## 涉及文件

| 文件 | 变更 |
|------|------|
| `peri-agent/src/goal/model.rs` | ThreadGoal 可能扩展字段（如 estimated 标记） |
| `peri-agent/src/goal/store.rs` | account_thread_goal_usage 事务逻辑保持，添加 fallback 友好错误处理 |
| `peri-agent/src/goal/steering.rs` | GoalSteering trait 可能新增 compact_reorient / budget_warning 模板 |
| `peri-acp/src/session/goal_state/mod.rs` | 新增 pending_user_message / last_injected_usage_pct / last_injected_wall_clock 字段；take_pending_user_message 方法；百分比检查逻辑 |
| `peri-acp/src/session/continuation.rs` | continuation loop 读取 pending_user_message 作为下一轮 content |
| `peri-acp/src/session/command/goal.rs` | 新增 edit / budget 子命令 |
| `peri-acp/src/session/handler.rs`（或 prompt handler） | 用户消息到达时写入 GoalState.pending_user_message（goal Active 期间） |
| `peri-middlewares/src/goal_middleware.rs` | 重构为事件性注入；新增 compact_just_happened / 百分比阈值读取；compact_reorient 模板 |
| `peri-middlewares/src/compact_middleware.rs` | after_model 设置 context 标志 `compact_just_happened` |
| `peri-middlewares/src/goal/templates/*.md` | 新增 compact_reorient.md / budget_warning.md / budget_urgent.md / periodic.md |
| `peri-middlewares/src/goal/tools.rs` | update_goal 新增 reason 字段（Blocked 必填）；get_goal 返回字段扩展 |
| `peri-middlewares/src/subagent/` | 新增 GoalAccountingMiddleware；SubAgent builder 分流 goal_state 注入策略 |
| `peri-tui/src/app/goal_mirror.rs` | 新增 injection_events 状态 + steering 通知解析 |
| `peri-tui/src/app/goal_panel.rs` | 形态 B：注入历史时间线；快捷键 s/e/b/p/r/c |
| `peri-tui/src/app/render/status_bar.rs` | 形态 C：goal status indicator + budget 颜色 |
| `peri-tui/src/app/render/`（对话流） | 形态 A：inline injection marker 渲染 |
| `peri-tui/src/acp_client/`（通知类型） | 新增 GoalSteeringInjected 变体 |
| `peri-acp/src/event/`（通知类型） | 新增 PeriNotification::GoalSteeringInjected |
| `langfuse-client/` | 扩展 trace_goal_event / trace_steering_injection 方法 |
| `peri-tui/src/app/agent_ops/lifecycle.rs` | 接收 GoalSteeringInjected 事件更新 goal_mirror |

## 变更记录

| 日期 | 变更 |
|------|------|
| 2026-06-12 | 初始架构审查，识别 8 个问题，提出可插拔架构设计方向 |
| 2026-06-12 | Grill 完成（17 个 branch），rebuild 设计决策确定；文档重构为"Rebuild 设计文档" |
| 2026-06-12 | 转换为 superpowers 规范格式；新增「需求」「不做什么」「涉及文件」段落 |
