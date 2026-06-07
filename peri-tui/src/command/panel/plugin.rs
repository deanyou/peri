use crate::{app::App, command::Command};

pub struct PluginCommand;

impl Command for PluginCommand {
    fn name(&self) -> &str {
        "plugin"
    }
    fn description(&self, _lc: &crate::i18n::LcRegistry) -> String {
        _lc.tr("command-plugin-description")
    }
    fn execute(&self, app: &mut App, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.as_slice() {
            // /plugin（无参数）→ 打开面板（现有行为）
            [] => app.open_plugin_panel(),

            // /plugin marketplace add <url>
            ["marketplace", "add", rest @ ..] if !rest.is_empty() => {
                let input = rest.join(" ");
                if let Err(e) = app.marketplace_add_and_save(&input) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("添加 marketplace 失败: {}", e));
                }
            }

            // /plugin install <name@marketplace>
            ["install", name_at_marketplace] => {
                let (name, marketplace) = name_at_marketplace
                    .split_once('@')
                    .unwrap_or((name_at_marketplace, "claude-plugins-official"));
                if let Err(e) = app.plugin_install_by_marketplace(name, marketplace) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("安装插件失败: {}", e));
                }
            }

            // /plugin marketplace update <name>
            ["marketplace", "update", name] => {
                if let Err(e) = app.marketplace_update_and_refresh(name) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("更新 marketplace 失败: {}", e));
                }
            }

            // 未知用法 → 显示帮助
            _ => {
                let help = "用法:\n\
                    /plugin                                    — 打开插件面板\n\
                    /plugin marketplace add <url>              — 添加市场源\n\
                    /plugin install <name>@<marketplace>       — 安装插件\n\
                    /plugin marketplace update <name>          — 更新市场缓存";
                app.session_mgr
                    .current_mut()
                    .messages
                    .push_system_note(help.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    include!("plugin_test.rs");
}
