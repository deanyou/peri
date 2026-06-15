use super::*;
use peri_agent::agent::state::AgentState;
use peri_agent::goal::{GoalStateView, GoalStatus, GoalViewSnapshot};
use std::sync::{Arc, Mutex};

/// Mock GoalStateView — 测试用，避免 peri-middlewares → peri-acp 依赖
struct MockGoalStateView {
    snapshot_data: Mutex<GoalViewSnapshot>,
    objective_just_updated: Mutex<bool>,
}

impl MockGoalStateView {
    fn new() -> Self {
        Self {
            snapshot_data: Mutex::new(GoalViewSnapshot::default()),
            objective_just_updated: Mutex::new(false),
        }
    }

    fn set_goal(&self, objective: &str, budget: Option<u64>) {
        let mut snap = self.snapshot_data.lock().unwrap();
        snap.objective = Some(objective.to_string());
        snap.status = Some(GoalStatus::Active);
        snap.token_budget = budget;
        *self.objective_just_updated.lock().unwrap() = true;
    }
}

impl GoalStateView for MockGoalStateView {
    fn snapshot(&self) -> GoalViewSnapshot {
        self.snapshot_data.lock().unwrap().clone()
    }

    fn consume_objective_updated(&self) -> bool {
        let mut guard = self.objective_just_updated.lock().unwrap();
        let was_set = *guard;
        *guard = false;
        was_set
    }
}

fn make_middleware() -> (GoalMiddleware, Arc<MockGoalStateView>) {
    let view: Arc<MockGoalStateView> = Arc::new(MockGoalStateView::new());
    let middleware = GoalMiddleware::new(view.clone());
    (middleware, view)
}

#[tokio::test]
async fn test_before_model_无_goal_不注入() {
    let (middleware, _view) = make_middleware();
    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    let initial_len = state.messages().len();

    middleware.before_model(&mut state).await.unwrap();

    assert_eq!(state.messages().len(), initial_len);
}

#[tokio::test]
async fn test_before_model_set_goal_后注入_steering() {
    let (middleware, view) = make_middleware();
    view.set_goal("完成模块重构", Some(200_000));

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();

    // 注入了一条 Human 消息
    assert_eq!(state.messages().len(), 1);
    let msg = &state.messages()[0];
    assert!(matches!(msg, BaseMessage::Human { .. }));
    let text = msg.content();
    assert!(text.contains("完成模块重构"));
    assert!(text.contains("200000"));
}

#[tokio::test]
async fn test_before_model_注入后_consume_objective_updated() {
    let (middleware, view) = make_middleware();
    view.set_goal("测试", None);

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();
    // objective_just_updated 应被消费
    assert!(!view.consume_objective_updated());
}

#[tokio::test]
async fn test_before_model_连续调用_不重复注入() {
    let (middleware, view) = make_middleware();
    view.set_goal("测试", None);

    let mut state = AgentState::with_messages("/tmp".to_string(), vec![]);
    middleware.before_model(&mut state).await.unwrap();
    middleware.before_model(&mut state).await.unwrap();

    // 第二次调用不应注入（objective_just_updated 已清零）
    assert_eq!(state.messages().len(), 1);
}
