# Goal Rebuild Plan 1a: 数据层基础 + Middleware 注入骨架

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从零搭建 goal 子系统的数据层（ThreadGoal/GoalStore/GoalState），实现 GoalMiddleware 的 set/objective_updated 注入骨架，并集成到 AcpSession——让 `/goal set` 能工作、agent 能看到 goal 注入。

**Architecture:** 三层分离——`peri-agent` 提供 pure data model + store trait（无 ACP 依赖），`peri-acp` 提供 GoalState 并发状态机（Arc<RwLock> + 停车场锁），`peri-middlewares` 提供 GoalMiddleware（before_model 钩子注入）。本计划不含 continuation loop（Plan 1b）、不含 Y 模型完整触发（Plan 1c）、不含计费 hybrid fallback（Plan 2）。

**Tech Stack:** Rust 2021 edition, tokio async/await + async-trait, parking_lot::RwLock（短锁无 await）, tempfile（测试隔离）, tracing（日志）

**参考实现:** 旧分支 `feat/goal-steering` 有完整的 goal 实现（已 revert），实施者可用 `git show feat/goal-steering:peri-agent/src/goal/model.rs` 等命令查看旧实现作为参考。但旧实现的注入策略是每轮注入，本计划改为事件性注入——**不要直接复制旧实现的注入逻辑**。

**关联 Spec:** `docs/superpowers/specs/2026-06-12-goal-architecture-rebuild-design.md`

**前置条件:**
- 当前分支 `feat/goal-rebuild`（从 `main` 干净起步，无 goal 代码）
- Spec 已审查通过

**关键设计约束：**

- **GoalState 写方法是 `async fn`**：`set_goal` / `clear` / `set_status_with_reason` / `flush_progress` 都标记为 `async`。原因：这些方法需要调用 `GoalStore`（async trait），如果用 `block_on` 在 tokio runtime 内会 panic（`/goal set` 命令在 tokio runtime 内执行）。spec 说的"短锁，无 await"指的是 `RwLock` 持有时间短（在 `.await` 之前 `drop` 锁），不是方法不能是 async。调用方（命令路径）本身是 async，`.await` 无障碍。
- **读方法保持同步**：`snapshot` / `consume_objective_updated` / `put_pending_user_message` / `take_pending_user_message` / `record_token_usage` / `record_time_usage` 是纯内存操作，保持同步。`GoalMiddleware::before_model` 只调用这些同步方法 + `GoalStateView` trait（同步），不受 async 写方法影响。
- **fire-and-forget vs await**：本计划写方法直接 `.await` store 操作（调用方能感知错误）。如果未来需要非阻塞路径，可在调用方用 `tokio::spawn` 包装。

---

## 文件结构

### 新建文件

| 文件 | 职责 | 行数估算 |
|------|------|---------|
| `peri-agent/src/goal/mod.rs` | goal 模块入口，re-export 公共 API | ~15 |
| `peri-agent/src/goal/model.rs` | `ThreadGoal` / `GoalStatus` / `GoalAccounting` 数据模型 | ~120 |
| `peri-agent/src/goal/model_test.rs` | 模型单元测试 | ~80 |
| `peri-agent/src/goal/store.rs` | `GoalStore` trait + `InMemoryGoalStore` | ~100 |
| `peri-agent/src/goal/store_test.rs` | store 测试 | ~90 |
| `peri-acp/src/session/goal_state/mod.rs` | `GoalState`（Arc<RwLock<GoalStateInner>>）+ 并发状态机 | ~280 |
| `peri-acp/src/session/goal_state/mod_test.rs` | GoalState 测试 | ~200 |
| `peri-middlewares/src/goal_middleware.rs` | `GoalMiddleware`（before_model 注入骨架） | ~180 |
| `peri-middlewares/src/goal_middleware_test.rs` | middleware 测试 | ~150 |

### 修改文件

| 文件 | 变更 |
|------|------|
| `peri-agent/src/lib.rs` | 添加 `pub mod goal;` |
| `peri-acp/src/session/mod.rs` | 添加 `pub mod goal_state;` + `AcpSession` 新增 `goal_state` 字段 |
| `peri-middlewares/src/lib.rs` | 添加 `pub mod goal_middleware;` + re-export |

### 不在本计划范围

- **continuation loop** — Plan 1b 实现（`pending_user_message` 的消费者）
- **机制 3 完整实现**（用户消息打断 continuation）— Plan 1b
- **compact_just_happened context 协调** — Plan 1b
- **Y 模型 T2-T6 触发**（budget_limit/百分比/compact_reorient）— Plan 1c
- **GoalState hydrate**（session resume 恢复）— Plan 1b
- **SQLite GoalStore** — Plan 2（本计划仅 InMemoryGoalStore）
- **计费 hybrid fallback**（char/4 估算）— Plan 2
- **/goal edit / /goal budget 子命令** — Plan 3
- **TUI 渲染**（形态 A/B/C）— Plan 3
- **Langfuse trace** — Plan 2
- **SubAgent 隔离** — Plan 4

---

## Task 1: ThreadGoal + GoalStatus 数据模型

**Files:**
- Create: `peri-agent/src/goal/mod.rs`
- Create: `peri-agent/src/goal/model.rs`
- Create: `peri-agent/src/goal/model_test.rs`
- Modify: `peri-agent/src/lib.rs`

- [ ] **Step 1: 在 `peri-agent/src/lib.rs` 中声明 goal 模块**

在 `peri-agent/src/lib.rs` 中找到现有的 `pub mod` 声明区域（通常在文件顶部），添加：

```rust
pub mod goal;
```

- [ ] **Step 2: 创建 `peri-agent/src/goal/mod.rs` 模块入口**

```rust
//! Goal Steering 子系统 — 长程目标跟踪 + 计费 + steering 注入。
//!
//! 本模块提供 pure data model 和 store trait，无 ACP/middleware 依赖。
//! 并发状态机见 `peri-acp::session::goal_state::GoalState`。

pub mod model;
pub mod store;

pub use model::{GoalAccounting, GoalStatus, ThreadGoal};
pub use store::{GoalStore, InMemoryGoalStore};
```

- [ ] **Step 3: 编写失败的测试 — ThreadGoal 构造与序列化**

创建 `peri-agent/src/goal/model_test.rs`：

```rust
use super::*;
use chrono::Utc;

#[test]
fn test_thread_goal_new_生成有效_goal_id() {
    let goal = ThreadGoal::new("完成 PR review".to_string(), None);
    assert_eq!(goal.objective, "完成 PR review");
    assert_eq!(goal.status, GoalStatus::Active);
    assert_eq!(goal.token_budget, None);
    assert!(!goal.goal_id.is_empty());
    assert!(goal.created_at <= Utc::now());
}

#[test]
fn test_thread_goal_with_budget() {
    let goal = ThreadGoal::new("重构模块".to_string(), Some(200_000));
    assert_eq!(goal.token_budget, Some(200_000));
}

#[test]
fn test_thread_goal_serde_roundtrip() {
    let goal = ThreadGoal::new("测试序列化".to_string(), Some(100_000));
    let json = serde_json::to_string(&goal).unwrap();
    let deserialized: ThreadGoal = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.objective, goal.objective);
    assert_eq!(deserialized.token_budget, goal.token_budget);
}

#[test]
fn test_goal_status_转换合法() {
    use GoalStatus::*;
    // Active 可以 → Paused / Complete / Blocked / BudgetLimited
    assert!(Active.can_transition_to(&Paused));
    assert!(Active.can_transition_to(&Complete));
    assert!(Active.can_transition_to(&Blocked));
    assert!(Active.can_transition_to(&BudgetLimited));
    // Paused 可以 → Active
    assert!(Paused.can_transition_to(&Active));
    // Complete 是终态，不能转换
    assert!(!Complete.can_transition_to(&Active));
}
```

- [ ] **Step 4: 运行测试验证失败**

Run: `cargo test -p peri-agent --lib goal::model_test`
Expected: FAIL — `cannot find type ThreadGoal in this scope`

- [ ] **Step 5: 实现 ThreadGoal + GoalStatus**

创建 `peri-agent/src/goal/model.rs`：

```rust
//! Goal 子系统的核心数据模型。
//!
//! `ThreadGoal` 是事实数据，必须跨 session 持久化。
//! `GoalStatus` 是状态机枚举，转换规则见 `can_transition_to`。
//! `GoalAccounting` 是计费状态（token/time 增量累积）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Goal 状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// 活跃，continuation 可运行
    Active,
    /// 用户暂停
    Paused,
    /// Agent 宣告完成
    Complete,
    /// Agent 宣告阻塞（必须附带 reason）
    Blocked,
    /// 预算耗尽
    BudgetLimited,
}

impl GoalStatus {
    /// 检查状态转换是否合法
    pub fn can_transition_to(&self, target: &GoalStatus) -> bool {
        use GoalStatus::*;
        match (self, target) {
            // 终态不可转换
            (Complete, _) | (Blocked, _) | (BudgetLimited, _) => false,
            // Active → 任意非 Active
            (Active, Paused | Complete | Blocked | BudgetLimited) => true,
            (Active, Active) => false,
            // Paused → Active（resume）
            (Paused, Active) => true,
            (Paused, _) => false,
        }
    }

    /// 是否是终态（continuation 应停止）
    pub fn is_terminal(&self) -> bool {
        use GoalStatus::*;
        matches!(self, Complete | Blocked | BudgetLimited)
    }
}

impl std::fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use GoalStatus::*;
        match self {
            Active => write!(f, "active"),
            Paused => write!(f, "paused"),
            Complete => write!(f, "complete"),
            Blocked => write!(f, "blocked"),
            BudgetLimited => write!(f, "budget_limited"),
        }
    }
}

/// 计费状态（累积增量）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalAccounting {
    /// 已用 token（含 input + output - cache_read）
    pub tokens_used: u64,
    /// 已用时间（秒）
    pub time_used_seconds: u64,
}

/// Thread-level goal 事实数据（持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadGoal {
    /// 唯一标识（uuid v7）
    pub goal_id: String,
    /// 目标描述
    pub objective: String,
    /// 当前状态
    pub status: GoalStatus,
    /// Token 预算上限（None = 无上限）
    pub token_budget: Option<u64>,
    /// 阻塞原因（仅 Blocked 状态有值）
    pub blocked_reason: Option<String>,
    /// 计费状态
    pub accounting: GoalAccounting,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
}

impl ThreadGoal {
    pub fn new(objective: String, token_budget: Option<u64>) -> Self {
        let now = Utc::now();
        Self {
            goal_id: uuid::Uuid::now_v7().to_string(),
            objective,
            status: GoalStatus::Active,
            token_budget,
            blocked_reason: None,
            accounting: GoalAccounting::default(),
            created_at: now,
            updated_at: now,
        }
    }

    /// usage 百分比（0.0-1.0），budget=None 时返回 None
    pub fn usage_pct(&self) -> Option<f32> {
        self.token_budget
            .filter(|&b| b > 0)
            .map(|b| self.accounting.tokens_used as f32 / b as f32)
    }
}

#[cfg(test)]
#[path = "model_test.rs"]
mod tests;
```

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-agent --lib goal::model_test`
Expected: PASS — 4 tests

- [ ] **Step 7: Commit**

```bash
git add peri-agent/src/lib.rs peri-agent/src/goal/
git commit -m "feat(goal): ThreadGoal + GoalStatus 数据模型

- GoalStatus 状态机（Active/Paused/Complete/Blocked/BudgetLimited）
- can_transition_to 强制合法转换（终态不可恢复）
- ThreadGoal 含 goal_id（uuid v7）/objective/budget/accounting
- usage_pct() 计算预算使用百分比"
```

---

## Task 2: GoalStore trait + InMemoryGoalStore

**Files:**
- Create: `peri-agent/src/goal/store.rs`
- Create: `peri-agent/src/goal/store_test.rs`
- Modify: `peri-agent/src/goal/mod.rs`（已在 Task 1 中声明 `pub mod store;`）

- [ ] **Step 1: 编写失败的测试 — InMemoryGoalStore CRUD**

创建 `peri-agent/src/goal/store_test.rs`：

```rust
use super::*;

#[tokio::test]
async fn test_in_memory_store_save_and_load() {
    let store = InMemoryGoalStore::new();
    let goal = ThreadGoal::new("测试目标".to_string(), Some(100_000));

    store.save("thread-1", goal.clone()).await.unwrap();

    let loaded = store.load("thread-1").await.unwrap();
    assert_eq!(loaded.unwrap().objective, "测试目标");
}

#[tokio::test]
async fn test_in_memory_store_load_missing_returns_none() {
    let store = InMemoryGoalStore::new();
    let result = store.load("missing-thread").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_in_memory_store_overwrite_on_save() {
    let store = InMemoryGoalStore::new();
    let goal1 = ThreadGoal::new("目标 1".to_string(), None);
    let goal2 = ThreadGoal::new("目标 2".to_string(), None);

    store.save("thread-1", goal1).await.unwrap();
    store.save("thread-1", goal2).await.unwrap();

    let loaded = store.load("thread-1").await.unwrap().unwrap();
    assert_eq!(loaded.objective, "目标 2");
}

#[tokio::test]
async fn test_in_memory_store_delete() {
    let store = InMemoryGoalStore::new();
    let goal = ThreadGoal::new("待删除".to_string(), None);
    store.save("thread-1", goal).await.unwrap();

    store.delete("thread-1").await.unwrap();
    let result = store.load("thread-1").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_in_memory_store_concurrent_access() {
    use std::sync::Arc;
    let store = Arc::new(InMemoryGoalStore::new());
    let mut handles = Vec::new();

    for i in 0..10 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let goal = ThreadGoal::new(format!("目标 {}", i), None);
            s.save(&format!("thread-{}", i), goal).await.unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    for i in 0..10 {
        assert!(store.load(&format!("thread-{}", i)).await.unwrap().is_some());
    }
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-agent --lib goal::store_test`
Expected: FAIL — `cannot find type InMemoryGoalStore`

- [ ] **Step 3: 实现 GoalStore trait + InMemoryGoalStore**

创建 `peri-agent/src/goal/store.rs`：

```rust
//! Goal 持久化存储抽象。
//!
//! `GoalStore` trait 定义 save/load/delete 接口，供 ACP 层注入。
//! `InMemoryGoalStore` 是测试和 fallback 用的纯内存实现。
//! SQLite 实现见 Plan 2。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use super::model::ThreadGoal;

/// Goal 持久化存储 trait
#[async_trait]
pub trait GoalStore: Send + Sync {
    /// 保存（upsert）goal 到指定 thread
    async fn save(&self, thread_id: &str, goal: ThreadGoal) -> Result<(), GoalStoreError>;

    /// 加载指定 thread 的 goal，无 goal 返回 None
    async fn load(&self, thread_id: &str) -> Result<Option<ThreadGoal>, GoalStoreError>;

    /// 删除指定 thread 的 goal
    async fn delete(&self, thread_id: &str) -> Result<(), GoalStoreError>;
}

/// Store 错误类型
#[derive(Debug, thiserror::Error)]
pub enum GoalStoreError {
    #[error("存储 IO 错误: {0}")]
    Io(String),
    #[error("序列化错误: {0}")]
    Serde(String),
}

/// 纯内存实现（测试 + fallback）
pub struct InMemoryGoalStore {
    inner: Arc<RwLock<HashMap<String, ThreadGoal>>>,
}

impl InMemoryGoalStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryGoalStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GoalStore for InMemoryGoalStore {
    async fn save(&self, thread_id: &str, goal: ThreadGoal) -> Result<(), GoalStoreError> {
        self.inner.write().insert(thread_id.to_string(), goal);
        Ok(())
    }

    async fn load(&self, thread_id: &str) -> Result<Option<ThreadGoal>, GoalStoreError> {
        Ok(self.inner.read().get(thread_id).cloned())
    }

    async fn delete(&self, thread_id: &str) -> Result<(), GoalStoreError> {
        self.inner.write().remove(thread_id);
        Ok(())
    }
}

#[cfg(test)]
#[path = "store_test.rs"]
mod tests;
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p peri-agent --lib goal::store_test`
Expected: PASS — 5 tests

- [ ] **Step 5: Commit**

```bash
git add peri-agent/src/goal/store.rs peri-agent/src/goal/store_test.rs
git commit -m "feat(goal): GoalStore trait + InMemoryGoalStore

- GoalStore trait（save/load/delete）+ GoalStoreError
- InMemoryGoalStore 用 parking_lot::RwLock 保护 HashMap
- 并发安全（Arc<RwLock>），5 个测试覆盖 CRUD + 并发"
```

---

## Task 3: GoalState 结构骨架 + set_goal/clear

**Files:**
- Create: `peri-acp/src/session/goal_state/mod.rs`
- Create: `peri-acp/src/session/goal_state/mod_test.rs`
- Modify: `peri-acp/src/session/mod.rs`

- [ ] **Step 1: 在 `peri-acp/src/session/mod.rs` 中声明 goal_state 模块**

找到 `peri-acp/src/session/mod.rs` 顶部的 `pub mod` 声明区域（当前约第 6-12 行），添加：

```rust
pub mod goal_state;
```

- [ ] **Step 2: 编写失败的测试 — set_goal / clear**

创建 `peri-acp/src/session/goal_state/mod_test.rs`：

```rust
use super::*;
use peri_agent::goal::{InMemoryGoalStore, ThreadGoal};

fn make_state() -> GoalState {
    GoalState::new(
        Arc::new(InMemoryGoalStore::new()),
        "test-thread".to_string(),
    )
}

#[tokio::test]
async fn test_set_goal_写入_store_并触发_objective_updated() {
    let state = make_state();
    state.set_goal("完成模块重构".to_string(), Some(200_000)).await.unwrap();

    let snap = state.snapshot();
    assert_eq!(snap.objective.as_deref(), Some("完成模块重构"));
    assert_eq!(snap.token_budget, Some(200_000));
    assert_eq!(snap.status, GoalStatus::Active);
    assert!(snap.objective_just_updated);
}

#[tokio::test]
async fn test_clear_清空_goal() {
    let state = make_state();
    state.set_goal("临时目标".to_string(), None).await.unwrap();
    state.clear().await.unwrap();

    let snap = state.snapshot();
    assert!(snap.objective.is_none());
    assert!(!snap.objective_just_updated);
}

#[tokio::test]
async fn test_set_goal_覆盖旧_goal_生成新_goal_id() {
    let state = make_state();
    state.set_goal("目标 A".to_string(), None).await.unwrap();
    let id_a = state.snapshot().goal_id.clone().unwrap();

    state.set_goal("目标 B".to_string(), None).await.unwrap();
    let id_b = state.snapshot().goal_id.clone().unwrap();

    assert_ne!(id_a, id_b);
    assert_eq!(state.snapshot().objective.as_deref(), Some("目标 B"));
}

#[tokio::test]
async fn test_store_写入失败_内存镜像仍可读() {
    use async_trait::async_trait;
    use peri_agent::goal::{GoalStore, GoalStoreError, ThreadGoal};

    struct FailingStore;
    #[async_trait]
    impl GoalStore for FailingStore {
        async fn save(&self, _: &str, _: ThreadGoal) -> Result<(), GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
        async fn load(&self, _: &str) -> Result<Option<ThreadGoal>, GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
        async fn delete(&self, _: &str) -> Result<(), GoalStoreError> {
            Err(GoalStoreError::Io("simulated".to_string()))
        }
    }

    let state = GoalState::new(Arc::new(FailingStore), "test-thread".to_string());
    // set_goal 即使 store 失败也不 panic（内存镜像更新成功）
    let result = state.set_goal("fallback".to_string(), None).await;
    // store 失败返回 Err，但内存镜像已更新
    assert!(result.is_err());
    assert_eq!(state.snapshot().objective.as_deref(), Some("fallback"));
}
```

- [ ] **Step 3: 运行测试验证失败**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: FAIL — `cannot find type GoalState`

- [ ] **Step 4: 实现 GoalState 骨架**

创建 `peri-acp/src/session/goal_state/mod.rs`：

```rust
//! GoalState — goal 子系统的并发状态机。
//!
//! 基于 `Arc<RwLock<GoalStateInner>>` + `parking_lot::RwLock`（短锁无 await）。
//! store 写入失败时退化为纯内存模式（snapshot 读仍可用），不阻塞 agent。
//!
//! 并发模型：read-and-reset + epoch（本 Task 先实现基础读写，account_progress 的
//! read-and-reset 在 Task 5 实现）。

use std::sync::Arc;

use parking_lot::RwLock;
use peri_agent::goal::{GoalStore, GoalStatus, ThreadGoal};

/// Goal 快照（只读视图，供 middleware / TUI 读取）
#[derive(Debug, Clone, Default)]
pub struct GoalSnapshot {
    pub goal_id: Option<String>,
    pub objective: Option<String>,
    pub status: Option<GoalStatus>,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    /// set_goal / edit 后置 true，middleware 注入后清零
    pub objective_just_updated: bool,
}

impl GoalSnapshot {
    /// 是否有活跃的 goal
    pub fn has_active_goal(&self) -> bool {
        self.status == Some(GoalStatus::Active)
    }
}

/// 内部可变状态（受 RwLock 保护）
struct GoalStateInner {
    goal: Option<ThreadGoal>,
    /// set_goal / clear_goal 后置 true，GoalMiddleware 注入后清零
    objective_just_updated: bool,
    store: Arc<dyn GoalStore>,
    thread_id: String,
}

/// 并发安全的状态句柄
#[derive(Clone)]
pub struct GoalState {
    inner: Arc<RwLock<GoalStateInner>>,
}

impl GoalState {
    pub fn new(store: Arc<dyn GoalStore>, thread_id: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(GoalStateInner {
                goal: None,
                objective_just_updated: false,
                store,
                thread_id,
            })),
        }
    }

    /// set_goal：UPSERT（新 goal_id），触发 objective_updated。
    /// store 写入失败不回滚内存镜像（内存优于 store 原则）。
    pub async fn set_goal(
        &self,
        objective: String,
        token_budget: Option<u64>,
    ) -> Result<(), peri_agent::goal::GoalStoreError> {
        let new_goal = ThreadGoal::new(objective, token_budget);
        let (thread_id, store) = {
            let mut guard = self.inner.write();
            guard.goal = Some(new_goal.clone());
            guard.objective_just_updated = true;
            (guard.thread_id.clone(), guard.store.clone())
        };

        // 短锁：lock 在块作用域结束即释放，store.save 是长操作但不持锁
        if let Err(e) = store.save(&thread_id, new_goal).await {
            tracing::warn!(error = %e, "GoalState: store save 失败，退化为纯内存模式");
            return Err(e);
        }
        Ok(())
    }

    /// clear：清空 goal
    pub async fn clear(&self) -> Result<(), peri_agent::goal::GoalStoreError> {
        let (thread_id, store) = {
            let mut guard = self.inner.write();
            guard.goal = None;
            guard.objective_just_updated = false;
            (guard.thread_id.clone(), guard.store.clone())
        };

        if let Err(e) = store.delete(&thread_id).await {
            tracing::warn!(error = %e, "GoalState: store delete 失败");
            return Err(e);
        }
        Ok(())
    }

    /// 只读快照（短锁，立即释放）
    pub fn snapshot(&self) -> GoalSnapshot {
        let guard = self.inner.read();
        match &guard.goal {
            Some(g) => GoalSnapshot {
                goal_id: Some(g.goal_id.clone()),
                objective: Some(g.objective.clone()),
                status: Some(g.status),
                token_budget: g.token_budget,
                tokens_used: g.accounting.tokens_used,
                time_used_seconds: g.accounting.time_used_seconds,
                objective_just_updated: guard.objective_just_updated,
            },
            None => GoalSnapshot {
                objective_just_updated: guard.objective_just_updated,
                ..Default::default()
            },
        }
    }

    /// 消费 objective_just_updated 标志（middleware 注入后调用）
    pub fn consume_objective_updated(&self) -> bool {
        let mut guard = self.inner.write();
        let was_set = guard.objective_just_updated;
        guard.objective_just_updated = false;
        was_set
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
```

- [ ] **Step 5: 验证 `chrono` 和 `uuid` 依赖在 `peri-acp/Cargo.toml` 中存在**

Run: `grep -E 'chrono|uuid' peri-acp/Cargo.toml`

这两个 crate 应已在 `peri-agent` 的依赖中传递可用，但 `peri-acp` 直接使用 `chrono::Utc::now()` 需确认。如果 grep 无结果，在 `[dependencies]` 添加：

```toml
chrono = { workspace = true }
uuid = { workspace = true }
```

如果 workspace 根 `Cargo.toml` 无 chrono/uuid 的 workspace 依赖声明，则用各自 crate 的版本号（参考 `peri-agent/Cargo.toml` 中的版本）。

运行 `cargo check -p peri-acp` 验证编译。

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: PASS — 4 tests

- [ ] **Step 7: Commit**

```bash
git add peri-acp/src/session/mod.rs peri-acp/src/session/goal_state/ peri-acp/Cargo.toml
git commit -m "feat(goal): GoalState 并发状态机 + set_goal/clear

- Arc<RwLock<GoalStateInner>> + parking_lot::RwLock（短锁无 await）
- GoalSnapshot 只读快照供 middleware/TUI 读取
- set_goal: UPSERT + 触发 objective_just_updated 标志
- clear: 清空 goal + 清零标志
- store 写入失败不回滚内存镜像（内存优于 store 原则）"
```

---

## Task 4: GoalState set_status + 状态转换规则

**Files:**
- Modify: `peri-acp/src/session/goal_state/mod.rs`
- Modify: `peri-acp/src/session/goal_state/mod_test.rs`

- [ ] **Step 1: 编写失败的测试 — set_status 合法/非法转换**

在 `peri-acp/src/session/goal_state/mod_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn test_set_status_合法转换_active_to_paused() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    assert_eq!(state.snapshot().status, Some(GoalStatus::Active));

    state.set_status(GoalStatus::Paused).await.unwrap();
    assert_eq!(state.snapshot().status, Some(GoalStatus::Paused));
}

#[tokio::test]
async fn test_set_status_非法转换_complete_to_active_返回错误() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    state.set_status(GoalStatus::Complete).await.unwrap();

    let result = state.set_status(GoalStatus::Active).await;
    assert!(result.is_err());
    // 状态未改变
    assert_eq!(state.snapshot().status, Some(GoalStatus::Complete));
}

#[tokio::test]
async fn test_set_status_blocked_必须附带_reason() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();

    let result = state.set_status(GoalStatus::Blocked).await;
    assert!(result.is_err(), "Blocked 必须附带 reason");

    state.set_status_with_reason(GoalStatus::Blocked, "缺少依赖".to_string()).await.unwrap();
    assert_eq!(state.snapshot().status, Some(GoalStatus::Blocked));
}

#[tokio::test]
async fn test_set_status_无_goal_返回错误() {
    let state = make_state();
    let result = state.set_status(GoalStatus::Paused).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resume_from_complete_返回错误() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    state.set_status(GoalStatus::Complete).await.unwrap();

    // Complete 是终态，不能 resume
    let result = state.set_status(GoalStatus::Active).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: FAIL — `no method named set_status / set_status_with_reason`

- [ ] **Step 3: 实现 set_status + set_status_with_reason**

在 `peri-acp/src/session/goal_state/mod.rs` 的 `impl GoalState` 块中（`clear` 方法之后）追加：

```rust
    /// set_status（简化封装，reason 为空字符串）
    pub async fn set_status(&self, target: GoalStatus) -> Result<(), String> {
        self.set_status_with_reason(target, String::new()).await
    }

    /// set_status 附带 reason（Blocked 必填）
    pub async fn set_status_with_reason(
        &self,
        target: GoalStatus,
        reason: String,
    ) -> Result<(), String> {
        let (thread_id, store, goal_clone) = {
            let mut guard = self.inner.write();
            let goal = guard
                .goal
                .as_mut()
                .ok_or_else(|| "无活跃 goal，无法 set_status".to_string())?;

            if !goal.status.can_transition_to(&target) {
                return Err(format!(
                    "非法状态转换: {:?} → {:?}（终态不可恢复）",
                    goal.status, target
                ));
            }

            // Blocked 必须附带 reason
            if matches!(target, GoalStatus::Blocked) && reason.trim().is_empty() {
                return Err("Blocked 状态必须附带 reason".to_string());
            }

            goal.status = target;
            goal.updated_at = chrono::Utc::now();
            if matches!(target, GoalStatus::Blocked) {
                goal.blocked_reason = Some(reason.clone());
            }
            (
                guard.thread_id.clone(),
                guard.store.clone(),
                goal.clone(),
            )
        };

        // best-effort store 写入（短锁已释放）
        let _ = store.save(&thread_id, goal_clone).await;
        Ok(())
    }
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: PASS — 9 tests（Task 3 的 4 个 + 本 Task 的 5 个）

- [ ] **Step 5: Commit**

```bash
git add peri-acp/src/session/goal_state/
git commit -m "feat(goal): GoalState set_status + 状态转换规则

- can_transition_to 强制合法转换（Complete/Blocked/BudgetLimited 是终态）
- Blocked 必须附带 reason（set_status_with_reason）
- 无 goal 时 set_status 返回错误
- best-effort store 写入（失败不阻塞状态变更）"
```

---

## Task 5: GoalState pending_user_message 字段

**Files:**
- Modify: `peri-acp/src/session/goal_state/mod.rs`
- Modify: `peri-acp/src/session/goal_state/mod_test.rs`

**注意:** 本 Task 只实现 `pending_user_message` 的数据结构和 put/take 方法。消费者（continuation loop）在 Plan 1b 实现。

- [ ] **Step 1: 编写失败的测试 — put/take + 清理时机**

在 `peri-acp/src/session/goal_state/mod_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn test_put_pending_user_message_覆盖旧值() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();

    state.put_pending_user_message("第一条".to_string());
    state.put_pending_user_message("第二条".to_string());

    let taken = state.take_pending_user_message();
    assert_eq!(taken.as_deref(), Some("第二条"));
    // take 后清空
    assert!(state.take_pending_user_message().is_none());
}

#[tokio::test]
async fn test_clear_goal_清零_pending_user_message() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    state.put_pending_user_message("待清空".to_string());

    state.clear().await.unwrap();
    assert!(state.take_pending_user_message().is_none());
}

#[tokio::test]
async fn test_set_status_complete_清零_pending_user_message() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    state.put_pending_user_message("待清空".to_string());

    state.set_status(GoalStatus::Complete).await.unwrap();
    assert!(state.take_pending_user_message().is_none());
}

#[tokio::test]
async fn test_set_status_paused_保留_pending_user_message() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();
    state.put_pending_user_message("保留".to_string());

    state.set_status(GoalStatus::Paused).await.unwrap();
    assert_eq!(
        state.take_pending_user_message().as_deref(),
        Some("保留")
    );
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: FAIL — `no method named put_pending_user_message / take_pending_user_message`

- [ ] **Step 3: 在 GoalStateInner 中添加 pending_user_message 字段**

修改 `peri-acp/src/session/goal_state/mod.rs`，在 `GoalStateInner` 结构体中添加字段：

```rust
struct GoalStateInner {
    goal: Option<ThreadGoal>,
    objective_just_updated: bool,
    store: Arc<dyn GoalStore>,
    thread_id: String,
    /// 机制 3：continuation 期间用户消息缓冲（多条覆盖，只保留最后一条）
    pending_user_message: Option<String>,
}
```

更新 `GoalState::new` 初始化：

```rust
    pub fn new(store: Arc<dyn GoalStore>, thread_id: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(GoalStateInner {
                goal: None,
                objective_just_updated: false,
                store,
                thread_id,
                pending_user_message: None,
            })),
        }
    }
```

- [ ] **Step 4: 实现 put / take 方法**

在 `impl GoalState` 块中（`consume_objective_updated` 之后）追加：

```rust
    /// 机制 3：写入用户消息（覆盖旧值）
    pub fn put_pending_user_message(&self, message: String) {
        self.inner.write().pending_user_message = Some(message);
    }

    /// 机制 3：取出并清空用户消息
    pub fn take_pending_user_message(&self) -> Option<String> {
        self.inner.write().pending_user_message.take()
    }
```

- [ ] **Step 5: 在 clear 和 set_status 中清零 pending_user_message**

修改 `clear` 方法，在 `guard.goal = None;` 之后添加：

```rust
            guard.pending_user_message = None;
```

修改 `set_status_with_reason` 方法，在 `goal.status = target;` 之后添加（终态清零，Paused 保留）：

```rust
            // 终态清零 pending_user_message（保留优于清除的反例：终态不需要用户消息）
            if target.is_terminal() {
                guard.pending_user_message = None;
            }
```

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: PASS — 13 tests

- [ ] **Step 7: Commit**

```bash
git add peri-acp/src/session/goal_state/
git commit -m "feat(goal): pending_user_message 字段（机制 3 数据层）

- put_pending_user_message: 覆盖式写入（多条只保留最后一条）
- take_pending_user_message: 取出并清空
- clear / set_status(终态) 时清零
- set_status(Paused) 时保留（暂停期间用户消息不丢失）
- 消费者（continuation loop）在 Plan 1b 实现"
```

---

## Task 6: GoalState account_progress 基础 flush

**Files:**
- Modify: `peri-acp/src/session/goal_state/mod.rs`
- Modify: `peri-acp/src/session/goal_state/mod_test.rs`

**注意:** 本 Task 实现基础 token 累加 + flush。hybrid fallback（char/4 估算）在 Plan 2 实现。百分比阈值检查（Y 模型 T5/T6）在 Plan 1c 实现。

- [ ] **Step 1: 编写失败的测试 — record + flush**

在 `peri-acp/src/session/goal_state/mod_test.rs` 末尾追加：

```rust
#[tokio::test]
async fn test_record_token_usage_累积到_pending() {
    let state = make_state();
    state.set_goal("测试".to_string(), Some(200_000)).await.unwrap();

    state.record_token_usage(1000);
    state.record_token_usage(500);

    // pending 累积 1500，但 snapshot 还没 flush
    // snapshot 读取的是已 flush 的值，所以仍是 0
    assert_eq!(state.snapshot().tokens_used, 0);
}

#[tokio::test]
async fn test_flush_progress_写入_goal_accounting() {
    let state = make_state();
    state.set_goal("测试".to_string(), Some(200_000)).await.unwrap();

    state.record_token_usage(1500);
    state.flush_progress().await.unwrap();

    assert_eq!(state.snapshot().tokens_used, 1500);
}

#[tokio::test]
async fn test_flush_progress_多次累加() {
    let state = make_state();
    state.set_goal("测试".to_string(), Some(200_000)).await.unwrap();

    state.record_token_usage(1000);
    state.flush_progress().await.unwrap();
    state.record_token_usage(500);
    state.flush_progress().await.unwrap();

    assert_eq!(state.snapshot().tokens_used, 1500);
}

#[tokio::test]
async fn test_record_time_usage_累积并_flush() {
    let state = make_state();
    state.set_goal("测试".to_string(), None).await.unwrap();

    state.record_time_usage(30);
    state.record_time_usage(15);
    state.flush_progress().await.unwrap();

    assert_eq!(state.snapshot().time_used_seconds, 45);
}

#[tokio::test]
async fn test_usage_pct_基于_flushed_值() {
    let state = make_state();
    state.set_goal("测试".to_string(), Some(200_000)).await.unwrap();

    state.record_token_usage(160_000);
    state.flush_progress().await.unwrap();

    let snap = state.snapshot();
    assert!((snap.usage_pct().unwrap() - 0.8).abs() < 0.01);
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: FAIL — `no method named record_token_usage / flush_progress`，`GoalSnapshot` 无 `usage_pct` 方法

- [ ] **Step 3: 在 GoalStateInner 中添加 pending 字段**

修改 `peri-acp/src/session/goal_state/mod.rs` 的 `GoalStateInner`：

```rust
struct GoalStateInner {
    goal: Option<ThreadGoal>,
    objective_just_updated: bool,
    store: Arc<dyn GoalStore>,
    thread_id: String,
    pending_user_message: Option<String>,
    /// 待 flush 的 token 增量
    pending_token_delta: u64,
    /// 待 flush 的 time 增量（秒）
    pending_time_delta_seconds: u64,
}
```

更新 `GoalState::new` 初始化添加：

```rust
                pending_token_delta: 0,
                pending_time_delta_seconds: 0,
```

- [ ] **Step 4: 实现 record + flush 方法**

在 `impl GoalState` 块中追加：

```rust
    /// 记录 token 增量到 pending 缓冲
    pub fn record_token_usage(&self, delta: u64) {
        self.inner.write().pending_token_delta += delta;
    }

    /// 记录时间增量到 pending 缓冲
    pub fn record_time_usage(&self, delta_seconds: u64) {
        self.inner.write().pending_time_delta_seconds += delta_seconds;
    }

    /// flush：将 pending 增量累加到 goal.accounting 并写 store
    pub async fn flush_progress(&self) -> Result<(), String> {
        let (thread_id, store, goal_clone) = {
            let mut guard = self.inner.write();
            let token_delta = std::mem::take(&mut guard.pending_token_delta);
            let time_delta = std::mem::take(&mut guard.pending_time_delta_seconds);

            let goal = match guard.goal.as_mut() {
                Some(g) => g,
                None => return Ok(()), // 无 goal，no-op
            };

            goal.accounting.tokens_used += token_delta;
            goal.accounting.time_used_seconds += time_delta;
            goal.updated_at = chrono::Utc::now();

            (
                guard.thread_id.clone(),
                guard.store.clone(),
                goal.clone(),
            )
        };

        // best-effort store 写入（短锁已释放）
        let _ = store.save(&thread_id, goal_clone).await;
        Ok(())
    }
```

- [ ] **Step 5: 在 GoalSnapshot 上添加 usage_pct 方法**

在 `GoalSnapshot` impl 块中追加：

```rust
    /// usage 百分比（0.0-1.0），budget=None 或 0 时返回 None
    pub fn usage_pct(&self) -> Option<f32> {
        self.token_budget
            .filter(|&b| b > 0)
            .map(|b| self.tokens_used as f32 / b as f32)
    }
```

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-acp --lib session::goal_state::mod_test`
Expected: PASS — 18 tests

- [ ] **Step 7: Commit**

```bash
git add peri-acp/src/session/goal_state/
git commit -m "feat(goal): account_progress 基础 flush（token + time）

- pending_token_delta / pending_time_delta_seconds 缓冲增量
- record_token_usage / record_time_usage 累积到 pending
- flush_progress: 原子累加到 goal.accounting + best-effort store 写入
- GoalSnapshot::usage_pct() 计算预算使用百分比
- hybrid fallback（char/4 估算）+ 百分比阈值注入在后续 Plan 实现"
```

---

## Task 7: GoalMiddleware before_model 注入骨架（set / objective_updated）

**Files:**
- Create: `peri-middlewares/src/goal_middleware.rs`
- Create: `peri-middlewares/src/goal_middleware_test.rs`
- Modify: `peri-middlewares/src/lib.rs`

- [ ] **Step 1: 在 `peri-middlewares/src/lib.rs` 中声明模块**

找到 `peri-middlewares/src/lib.rs` 中的 `pub mod` / `mod` 声明区域，添加：

```rust
pub mod goal_middleware;

pub use goal_middleware::GoalMiddleware;
```

- [ ] **Step 2: 编写失败的测试 — before_model 注入 set 模板**

创建 `peri-middlewares/src/goal_middleware_test.rs`：

```rust
use super::*;
use peri_acp::session::goal_state::GoalState;
use peri_agent::goal::InMemoryGoalStore;
use std::sync::Arc;

fn make_middleware() -> (GoalMiddleware, GoalState) {
    let goal_state = GoalState::new(
        Arc::new(InMemoryGoalStore::new()),
        "test-thread".to_string(),
    );
    let middleware = GoalMiddleware::new(goal_state.clone());
    (middleware, goal_state)
}

#[tokio::test]
async fn test_before_model_无_goal_不注入() {
    use peri_agent::agent::state::AgentState;
    let (middleware, _goal_state) = make_middleware();
    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    let initial_len = state.messages().len();

    middleware.before_model(&mut state).await.unwrap();

    assert_eq!(state.messages().len(), initial_len);
}

#[tokio::test]
async fn test_before_model_set_goal_后注入_steering() {
    use peri_agent::agent::state::AgentState;
    let (middleware, goal_state) = make_middleware();
    goal_state.set_goal("完成模块重构".to_string(), Some(200_000)).unwrap();

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();

    // 注入了一条 Human 消息
    assert_eq!(state.messages().len(), 1);
    let msg = &state.messages()[0];
    assert!(msg.is_human());
    let text = msg.text_content();
    assert!(text.contains("完成模块重构"));
    assert!(text.contains("200000"));
}

#[tokio::test]
async fn test_before_model_注入后_consume_objective_updated() {
    use peri_agent::agent::state::AgentState;
    let (middleware, goal_state) = make_middleware();
    goal_state.set_goal("测试".to_string(), None).unwrap();

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();
    // objective_just_updated 应被消费
    assert!(!goal_state.snapshot().objective_just_updated);
}

#[tokio::test]
async fn test_before_model_连续调用_不重复注入() {
    use peri_agent::agent::state::AgentState;
    let (middleware, goal_state) = make_middleware();
    goal_state.set_goal("测试".to_string(), None).unwrap();

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();
    middleware.before_model(&mut state).await.unwrap();

    // 第二次调用不应注入（objective_just_updated 已清零）
    assert_eq!(state.messages().len(), 1);
}
```

- [ ] **Step 3: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib goal_middleware_test`
Expected: FAIL — `cannot find type GoalMiddleware`

- [ ] **Step 4: 检查 `peri-middlewares/Cargo.toml` 是否有 `peri-acp` 依赖**

Run: `grep 'peri-acp' peri-middlewares/Cargo.toml`

如果没有，**不要**添加——`peri-middlewares` 不能依赖 `peri-acp`（workspace 禁止下层依赖上层，见 CLAUDE.md）。`GoalState` 定义在 `peri-acp`，而 `GoalMiddleware` 在 `peri-middlewares`，这会导致循环依赖。

**修正方案:** 将 `GoalMiddleware` 的依赖改为 trait 抽象。在 `peri-agent` 中定义 `GoalStateView` trait，`GoalState` 实现它，`GoalMiddleware` 依赖 trait 而非具体类型。

创建 `peri-agent/src/goal/view.rs`：

```rust
//! GoalStateView trait — 供 GoalMiddleware 依赖注入的抽象接口。
//!
//! GoalState（peri-acp）实现此 trait，GoalMiddleware（peri-middlewares）
//! 依赖 trait 而非具体类型，避免 peri-middlewares → peri-acp 循环依赖。

use super::model::GoalStatus;

/// 只读快照（与 GoalSnapshot 平行，但定义在 peri-agent 层避免依赖）
#[derive(Debug, Clone, Default)]
pub struct GoalViewSnapshot {
    pub objective: Option<String>,
    pub status: Option<GoalStatus>,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub objective_just_updated: bool,
}

/// GoalState 的抽象视图（供 middleware 依赖注入）
pub trait GoalStateView: Send + Sync {
    /// 只读快照
    fn snapshot(&self) -> GoalViewSnapshot;

    /// 消费 objective_just_updated 标志
    fn consume_objective_updated(&self) -> bool;
}
```

更新 `peri-agent/src/goal/mod.rs`：

```rust
pub mod model;
pub mod store;
pub mod view;

pub use model::{GoalAccounting, GoalStatus, ThreadGoal};
pub use store::{GoalStore, InMemoryGoalStore};
pub use view::{GoalStateView, GoalViewSnapshot};
```

- [ ] **Step 5: 为 GoalState 实现 GoalStateView trait**

在 `peri-acp/src/session/goal_state/mod.rs` 中添加 trait 实现：

```rust
impl peri_agent::goal::GoalStateView for GoalState {
    fn snapshot(&self) -> peri_agent::goal::GoalViewSnapshot {
        let snap = self.snapshot();
        peri_agent::goal::GoalViewSnapshot {
            objective: snap.objective,
            status: snap.status,
            token_budget: snap.token_budget,
            tokens_used: snap.tokens_used,
            objective_just_updated: snap.objective_just_updated,
        }
    }

    fn consume_objective_updated(&self) -> bool {
        self.consume_objective_updated()
    }
}
```

同时添加 `From` 转换或构造方法让 `GoalMiddleware` 可以接收 `Arc<dyn GoalStateView>`。更新 `GoalState` 的使用方式——在 `GoalMiddleware` 中持有 `Arc<dyn GoalStateView>`。

- [ ] **Step 6: 实现 GoalMiddleware**

创建 `peri-middlewares/src/goal_middleware.rs`：

```rust
//! GoalMiddleware — goal steering 注入骨架。
//!
//! before_model 钩子检查 objective_just_updated 标志，注入 set/updated 模板。
//! 本 Task 仅实现 T1（set/objective_updated）注入，T2-T6 在 Plan 1c 实现。
//!
//! 注入路径：add_message(Human, <system-reminder>) 尾部追加。
//! 绝不破坏 frozen_system_prompt（注入在 frozen 边界之外）。

use std::sync::Arc;

use async_trait::async_trait;
use peri_agent::{
    agent::state::State,
    error::AgentResult,
    messages::BaseMessage,
    middleware::r#trait::Middleware,
};

/// Goal steering 注入中间件
pub struct GoalMiddleware {
    goal_state: Arc<dyn peri_agent::goal::GoalStateView>,
}

impl GoalMiddleware {
    pub fn new(goal_state: Arc<dyn peri_agent::goal::GoalStateView>) -> Self {
        Self { goal_state }
    }

    /// 渲染 set/objective_updated 模板
    fn render_set_template(
        objective: &str,
        token_budget: Option<u64>,
        tokens_used: u64,
    ) -> String {
        let budget_line = match token_budget {
            Some(b) => format!("\n预算: {} tokens（已用 {}）", b, tokens_used),
            None => "\n预算: 无上限".to_string(),
        };
        format!(
            "<system-reminder>\n\
             [Goal Steering]\n\
             当前目标: {objective}\n\
             {budget_line}\n\
             \n\
             请围绕此目标持续推进。完成时用 update_goal(Complete) 声明；\
             遇到阻塞用 update_goal(Blocked, reason) 声明。\n\
             </system-reminder>"
        )
    }
}

#[async_trait]
impl<S: State> Middleware<S> for GoalMiddleware {
    fn name(&self) -> &str {
        "GoalMiddleware"
    }

    async fn before_model(&self, state: &mut S) -> AgentResult<()> {
        // T1: set / objective_updated 事件性注入
        if !self.goal_state.consume_objective_updated() {
            return Ok(());
        }

        let snap = self.goal_state.snapshot();
        let objective = match snap.objective.as_deref() {
            Some(o) => o,
            None => return Ok(()),
        };

        let template = Self::render_set_template(
            objective,
            snap.token_budget,
            snap.tokens_used,
        );
        state.add_message(BaseMessage::human(template));
        tracing::debug!(
            objective = %objective,
            "GoalMiddleware: 注入 set/objective_updated steering"
        );

        Ok(())
    }
}

#[cfg(test)]
#[path = "goal_middleware_test.rs"]
mod tests;
```

- [ ] **Step 7: 更新测试中的 make_middleware 辅助函数**

由于 `GoalMiddleware::new` 现在接收 `Arc<dyn GoalStateView>`，更新 `peri-middlewares/src/goal_middleware_test.rs` 的 `make_middleware`：

```rust
fn make_middleware() -> (GoalMiddleware, GoalState) {
    let goal_state = GoalState::new(
        Arc::new(InMemoryGoalStore::new()),
        "test-thread".to_string(),
    );
    let view: Arc<dyn peri_agent::goal::GoalStateView> = Arc::new(goal_state.clone());
    let middleware = GoalMiddleware::new(view);
    (middleware, goal_state)
}
```

- [ ] **Step 8: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib goal_middleware_test`
Expected: PASS — 4 tests

Run: `cargo test -p peri-agent --lib goal`
Expected: PASS（GoalStateView trait 编译通过）

Run: `cargo test -p peri-acp --lib session::goal_state`
Expected: PASS（GoalState 实现 GoalStateView trait 编译通过）

- [ ] **Step 9: Commit**

```bash
git add peri-agent/src/goal/ peri-acp/src/session/goal_state/ peri-middlewares/src/
git commit -m "feat(goal): GoalMiddleware before_model 注入骨架（T1: set/updated）

- peri-agent/goal/view.rs: GoalStateView trait + GoalViewSnapshot
  （避免 peri-middlewares → peri-acp 循环依赖）
- GoalState 实现 GoalStateView trait
- GoalMiddleware::before_model 检查 objective_just_updated，注入 set 模板
- 注入路径: add_message(Human, <system-reminder>) 尾部追加
- T2-T6（budget_limit/百分比/compact_reorient）在 Plan 1c 实现"
```

---

## Task 8: AcpSession 集成 goal_state 字段

**Files:**
- Modify: `peri-acp/src/session/mod.rs`

- [ ] **Step 1: 编写失败的测试 — AcpSession 含 goal_state 字段**

检查 `peri-acp` 是否有 session 构造的集成测试。如果没有合适的测试入口，跳到 Step 2 直接修改源码并依赖类型检查。

添加字段到 `AcpSession` 结构体：

- [ ] **Step 2: 在 AcpSession 中添加 goal_state 字段**

修改 `peri-acp/src/session/mod.rs` 的 `AcpSession` 结构体（约第 36-53 行），在 `active_agents` 字段之后添加：

```rust
pub struct AcpSession {
    pub session_id: String,
    pub thread_id: ThreadId,
    pub cwd: String,
    pub cancel_token: CancellationToken,
    pub state_messages: Vec<BaseMessage>,
    pub created_at: chrono::DateTime<Utc>,
    pub provider_id: String,
    pub model_alias: String,
    pub permission_mode: Arc<SharedPermissionMode>,
    pub thinking: Option<ThinkingConfig>,
    pub active_agents: HashMap<ThreadId, AgentRuntime>,
    /// Goal steering 状态（session 级，跨 prompt 共享）
    pub goal_state: crate::session::goal_state::GoalState,
}
```

- [ ] **Step 3: 更新所有 AcpSession 构造点**

在 `peri-acp/src/session/mod.rs` 中搜索所有构造 `AcpSession { ... }` 的位置（`build_session` 和 `new_session_with_settings` 方法中）。为每个构造点添加：

```rust
goal_state: crate::session::goal_state::GoalState::new(
    Arc::new(peri_agent::goal::InMemoryGoalStore::new()),
    session_id.to_string(),
),
```

**注意:** `session_id` 在构造时已知。`InMemoryGoalStore` 是临时实现，SQLite 实现在 Plan 2 替换。

- [ ] **Step 4: 运行编译验证**

Run: `cargo check -p peri-acp`
Expected: 编译通过

如果出现 `goal_state` 字段缺失错误，检查是否所有构造点都已更新。

- [ ] **Step 5: 运行全部测试验证无回归**

Run: `cargo test -p peri-acp --lib`
Expected: PASS — 所有现有测试通过

- [ ] **Step 6: Commit**

```bash
git add peri-acp/src/session/mod.rs
git commit -m "feat(goal): AcpSession 集成 goal_state 字段

- AcpSession 新增 goal_state: GoalState 字段
- build_session / new_session_with_settings 初始化空 goal_state
- InMemoryGoalStore 临时实现（SQLite 替换在 Plan 2）"
```

---

## Task 9: 验证全链路编译 + 集成测试

**Files:**
- 无新文件，仅运行验证

- [ ] **Step 1: 运行全 workspace 编译**

Run: `cargo build`
Expected: 编译通过，无错误

- [ ] **Step 2: 运行全 workspace 测试**

Run: `cargo test`
Expected: 所有测试通过

- [ ] **Step 3: 运行 clippy 检查**

Run: `cargo clippy -- -D warnings`
Expected: 无 warning（如果有，按提示修复）

- [ ] **Step 4: 运行 fmt 检查**

Run: `cargo fmt --check`
Expected: 无 diff（如果有，运行 `cargo fmt` 修复）

- [ ] **Step 5: 验证 lefthook pre-commit 全部通过**

Run: `lefthook run pre-commit`
Expected: fmt / clippy / check / typos 全部通过

- [ ] **Step 6: Commit 验证结果（如果有修复）**

```bash
git add -A
git commit -m "chore(goal): 全链路编译 + 测试验证通过"
```

如果无需修复则跳过此步骤。

---

## 完成标准

Plan 1a 完成后应满足：

1. **`/goal set` 可工作**（命令实现在 Plan 3，但数据层已就绪）
2. **GoalMiddleware 在 set_goal 后注入一条 steering 消息**（T1 事件性注入）
3. **连续 before_model 调用不重复注入**（objective_just_updated 标志去重）
4. **pending_user_message 数据结构就绪**（消费者在 Plan 1b）
5. **account_progress 基础 flush 可用**（hybrid fallback 在 Plan 2）
6. **AcpSession 持有 goal_state，跨 prompt 共享**
7. **全 workspace 编译 + 测试 + clippy + fmt 通过**

## 后续计划依赖

- **Plan 1b**（continuation loop + 机制 3）依赖本计划的 GoalState.pending_user_message + GoalMiddleware
- **Plan 1c**（Y 模型 T2-T6）依赖本计划的 GoalMiddleware 骨架
- **Plan 2**（计费 + SQLite store + Langfuse）依赖本计划的 GoalStore trait + account_progress
- **Plan 3**（TUI + 命令）依赖本计划的 GoalState.snapshot
- **Plan 4**（SubAgent 隔离）依赖本计划的 GoalMiddleware

## 变更记录

| 日期 | 变更 |
|------|------|
| 2026-06-12 | 初始版本，从 spec Phase 1 拆分出数据层基础 |
