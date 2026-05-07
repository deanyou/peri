use crate::app::App;
use crate::command::Command;

/// /hooks 命令：打开 Hooks 查看面板
pub struct HooksCommand;

impl Command for HooksCommand {
    fn name(&self) -> &str {
        "hooks"
    }

    fn description(&self) -> &str {
        "/hooks - 查看 Hook 配置"
    }

    fn execute(&self, app: &mut App, _args: &str) {
        app.open_hooks_panel();
    }
}
