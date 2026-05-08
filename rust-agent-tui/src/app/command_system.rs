use rust_agent_middlewares::prelude::SkillMetadata;

use crate::command::CommandRegistry;

/// 命令系统：命令注册表、帮助列表、Skills 元数据。
pub struct CommandSystem {
    pub command_registry: CommandRegistry,
    pub command_help_list: Vec<(String, String, Vec<String>)>,
    pub skills: Vec<SkillMetadata>,
}

impl CommandSystem {
    pub fn new(command_registry: CommandRegistry, skills: Vec<SkillMetadata>) -> Self {
        let command_help_list: Vec<(String, String, Vec<String>)> = command_registry
            .list()
            .into_iter()
            .map(|(n, d, a)| {
                (
                    n.to_string(),
                    d.to_string(),
                    a.into_iter().map(String::from).collect(),
                )
            })
            .collect();
        Self {
            command_registry,
            command_help_list,
            skills,
        }
    }
}
