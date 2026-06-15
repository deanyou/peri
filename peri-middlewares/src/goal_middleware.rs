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
    agent::state::State, error::AgentResult, messages::BaseMessage, middleware::r#trait::Middleware,
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
    fn render_set_template(objective: &str, token_budget: Option<u64>, tokens_used: u64) -> String {
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

        let template = Self::render_set_template(objective, snap.token_budget, snap.tokens_used);
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
