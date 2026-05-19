use super::message_pipeline::PipelineAction;
use super::*;
use peri_agent::agent::events::CompactFileInfo;

impl App {
    pub(crate) fn handle_compact_started(&mut self) -> (bool, bool, bool) {
        let vm = MessageViewModel::system(self.services.lc.tr("app-compact-started"));
        self.apply_pipeline_action(PipelineAction::AddMessage(vm));
        (true, false, false)
    }

    pub(crate) fn handle_compact_completed(
        &mut self,
        _summary: String,
        files: Vec<CompactFileInfo>,
        skills: Vec<String>,
        micro_cleared: usize,
    ) -> (bool, bool, bool) {
        if micro_cleared > 0 {
            let vm = MessageViewModel::system(self.services.lc.tr_args(
                "app-compact-auto-cleared",
                &[("count".into(), (micro_cleared as i64).into())],
            ));
            self.apply_pipeline_action(PipelineAction::AddMessage(vm));
            return (true, false, false);
        }

        let mut label_lines = vec![format!("✻ {}", self.services.lc.tr("app-compact-done"))];
        for f in &files {
            label_lines.push(format!("  ⎿  Read {} ({} lines)", f.path, f.lines));
        }
        if !skills.is_empty() {
            label_lines.push(format!("  ⎿  Skill: {}", skills.join(", ")));
        }
        let compact_label = label_lines.join("\n");

        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .ephemeral_notes
            .clear();
        let view_msgs = vec![MessageViewModel::system(compact_label)];
        self.apply_pipeline_action(PipelineAction::RebuildAll {
            prefix_len: 0,
            tail_vms: view_msgs,
        });

        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pre_compact_token_snapshot = None;

        (true, false, false)
    }

    pub(crate) fn handle_compact_error(&mut self, msg: String) -> (bool, bool, bool) {
        let vm = MessageViewModel::system(
            self.services
                .lc
                .tr_args("app-compact-failed", &[("error".into(), msg.into())]),
        );
        self.apply_pipeline_action(PipelineAction::AddMessage(vm));

        if let Some(snapshot) = self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pre_compact_token_snapshot
            .take()
        {
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .session_token_tracker = snapshot;
        }

        (true, false, false)
    }
}
