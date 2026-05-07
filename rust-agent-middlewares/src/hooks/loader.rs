use std::fs;
use std::path::Path;

use crate::hooks::types::{HooksConfig, RegisteredHook};
use crate::plugin::types::PluginManifest;

/// Extract hooks config from a plugin.
///
/// Priority:
/// 1. `hooks/hooks.json` file in plugin install directory
/// 2. `hooks` field in `plugin.json` manifest
pub(crate) fn extract_hooks(manifest: &PluginManifest, install_path: &Path) -> Option<HooksConfig> {
    // Priority 1: hooks/hooks.json file
    let hooks_file = install_path.join("hooks").join("hooks.json");
    if hooks_file.exists() {
        if let Ok(content) = fs::read_to_string(&hooks_file) {
            if let Ok(config) = serde_json::from_str::<HooksConfig>(&content) {
                return Some(config);
            }
        }
    }

    // Priority 2: plugin.json hooks field
    manifest.hooks.clone()
}

/// Load hooks from `{cwd}/.claude/settings.local.json` `hooks` field.
///
/// Returns a list of `RegisteredHook` with `plugin_name = "settings.local.json"`.
pub fn load_settings_local_hooks(cwd: &str) -> Vec<RegisteredHook> {
    let settings_path = Path::new(cwd).join(".claude").join("settings.local.json");
    if !settings_path.exists() {
        tracing::debug!("No settings.local.json at {}", settings_path.display());
        return Vec::new();
    }

    let content = match fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read {}: {}", settings_path.display(), e);
            return Vec::new();
        }
    };

    // Parse the top-level JSON to extract the `hooks` field
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse {}: {}", settings_path.display(), e);
            return Vec::new();
        }
    };

    let hooks_value = match value.get("hooks") {
        Some(h) if h.is_object() => h,
        _ => return Vec::new(),
    };

    let hooks_config: HooksConfig = match serde_json::from_value(hooks_value.clone()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "Failed to parse hooks config from {}: {}",
                settings_path.display(),
                e
            );
            return Vec::new();
        }
    };

    let mut hooks = Vec::new();
    for (event, rules) in &hooks_config {
        for rule in rules {
            for hook_def in &rule.hooks {
                hooks.push(RegisteredHook {
                    hook: hook_def.clone(),
                    event: event.clone(),
                    matcher: rule
                        .matcher
                        .clone()
                        .or_else(|| hook_def.get_matcher().cloned()),
                    plugin_name: "settings.local.json".to_string(),
                    plugin_id: "settings.local".to_string(),
                    plugin_root: Path::new(cwd).to_path_buf(),
                    plugin_data_dir: Path::new(cwd).join(".claude"),
                    plugin_options: std::collections::HashMap::new(),
                });
            }
        }
    }

    tracing::info!(
        "Loaded {} hooks from settings.local.json ({} events)",
        hooks.len(),
        hooks_config.len()
    );

    hooks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::types::HookEvent;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_manifest_with_hooks(hooks: Option<HooksConfig>) -> PluginManifest {
        PluginManifest {
            name: "test-plugin".into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: None,
            commands: None,
            agents: None,
            skills: None,
            hooks,
            mcp_servers: None,
            lsp_servers: None,
            output_styles: None,
            channels: None,
            options: None,
            settings: None,
        }
    }

    #[test]
    fn test_file_priority_over_manifest() {
        let dir = tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        // File has PreToolUse
        let file_config = r#"{
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "echo file-hook"}]
                }
            ]
        }"#;
        std::fs::write(hooks_dir.join("hooks.json"), file_config).unwrap();

        // Manifest has PostToolUse
        let mut manifest_hooks: HooksConfig = HashMap::new();
        manifest_hooks.insert(crate::hooks::types::HookEvent::PostToolUse, vec![]);
        let manifest = make_manifest_with_hooks(Some(manifest_hooks));

        let result = extract_hooks(&manifest, dir.path()).unwrap();
        assert!(result.contains_key(&crate::hooks::types::HookEvent::PreToolUse));
        assert!(!result.contains_key(&crate::hooks::types::HookEvent::PostToolUse));
    }

    #[test]
    fn test_fallback_to_manifest_hooks() {
        let dir = tempdir().unwrap();
        // No hooks/hooks.json file

        let mut manifest_hooks: HooksConfig = HashMap::new();
        manifest_hooks.insert(crate::hooks::types::HookEvent::SessionStart, vec![]);
        let manifest = make_manifest_with_hooks(Some(manifest_hooks));

        let result = extract_hooks(&manifest, dir.path()).unwrap();
        assert!(result.contains_key(&crate::hooks::types::HookEvent::SessionStart));
    }

    #[test]
    fn test_both_missing_returns_none() {
        let dir = tempdir().unwrap();
        let manifest = make_manifest_with_hooks(None);

        let result = extract_hooks(&manifest, dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_json_falls_back_to_manifest() {
        let dir = tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        // Invalid JSON in hooks.json
        std::fs::write(hooks_dir.join("hooks.json"), "not valid json").unwrap();

        let mut manifest_hooks: HooksConfig = HashMap::new();
        manifest_hooks.insert(crate::hooks::types::HookEvent::Stop, vec![]);
        let manifest = make_manifest_with_hooks(Some(manifest_hooks));

        // Should fall back to manifest hooks
        let result = extract_hooks(&manifest, dir.path()).unwrap();
        assert!(result.contains_key(&crate::hooks::types::HookEvent::Stop));
    }

    #[test]
    fn test_empty_hooks_returns_empty_hashmap() {
        let dir = tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        std::fs::write(hooks_dir.join("hooks.json"), "{}").unwrap();

        let manifest = make_manifest_with_hooks(None);
        let result = extract_hooks(&manifest, dir.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_settings_local_hooks_basic() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "hooks": [
                            {"type": "command", "command": "echo pre"}
                        ]
                    }
                ],
                "Notification": [
                    {
                        "hooks": [
                            {"type": "command", "command": "echo notify"}
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            claude_dir.join("settings.local.json"),
            serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();

        let hooks = load_settings_local_hooks(dir.path().to_str().unwrap());
        assert_eq!(hooks.len(), 2);

        // Verify plugin source
        for h in &hooks {
            assert_eq!(h.plugin_name, "settings.local.json");
        }

        // Check both events are present (order not guaranteed)
        let has_pre = hooks
            .iter()
            .any(|h| matches!(&h.event, HookEvent::PreToolUse));
        let has_notification = hooks
            .iter()
            .any(|h| matches!(&h.event, HookEvent::Notification));
        assert!(has_pre, "should have PreToolUse hook");
        assert!(has_notification, "should have Notification hook");
    }

    #[test]
    fn test_load_settings_local_hooks_no_file() {
        let hooks = load_settings_local_hooks("/nonexistent/path");
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_settings_local_hooks_no_hooks_field() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("settings.local.json"), "{}").unwrap();

        let hooks = load_settings_local_hooks(dir.path().to_str().unwrap());
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_load_settings_local_hooks_with_matcher() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let settings = serde_json::json!({
            "hooks": {
                "FileChanged": [
                    {
                        "matcher": ".env|.env.local",
                        "hooks": [
                            {"type": "command", "command": "echo changed"}
                        ]
                    }
                ]
            }
        });
        std::fs::write(
            claude_dir.join("settings.local.json"),
            serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();

        let hooks = load_settings_local_hooks(dir.path().to_str().unwrap());
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].matcher.as_deref(), Some(".env|.env.local"));
    }

    #[test]
    fn test_load_from_real_project_dir() {
        // Test loading from the actual perihelion project directory
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let settings_path = std::path::Path::new(&cwd)
            .join(".claude")
            .join("settings.local.json");
        if !settings_path.exists() {
            eprintln!(
                "Skipping: no settings.local.json at {}",
                settings_path.display()
            );
            return;
        }
        let hooks = load_settings_local_hooks(&cwd);
        assert!(
            !hooks.is_empty(),
            "Should load hooks from project settings.local.json"
        );
        // Should have hooks for known events
        let has_pre = hooks
            .iter()
            .any(|h| matches!(&h.event, HookEvent::PreToolUse));
        let has_perm = hooks
            .iter()
            .any(|h| matches!(&h.event, HookEvent::PermissionRequest));
        assert!(has_pre, "Should have PreToolUse hook");
        assert!(has_perm, "Should have PermissionRequest hook");
    }
}
