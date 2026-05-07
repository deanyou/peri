use std::collections::HashSet;
use std::path::Path;

/// 替换插件路径变量和 $ARGUMENTS
///
/// 支持的变量：
/// - `${CLAUDE_PLUGIN_ROOT}` / `$CLAUDE_PLUGIN_ROOT` → 插件安装路径
/// - `${CLAUDE_PLUGIN_DATA}` / `$CLAUDE_PLUGIN_DATA` → 插件数据路径
/// - `${ARGUMENTS}` / `$ARGUMENTS` → 参数值
pub fn resolve_hook_variables(
    input: &str,
    plugin_root: &Path,
    plugin_data_dir: &Path,
    arguments: &str,
) -> String {
    let mut result = input.to_string();

    // 替换 ${CLAUDE_PLUGIN_ROOT} 和 $CLAUDE_PLUGIN_ROOT
    let root_str = path_to_posix(plugin_root);
    result = result.replace("${CLAUDE_PLUGIN_ROOT}", &root_str);
    result = result.replace("$CLAUDE_PLUGIN_ROOT", &root_str);

    // 替换 ${CLAUDE_PLUGIN_DATA} 和 $CLAUDE_PLUGIN_DATA
    let data_str = path_to_posix(plugin_data_dir);
    result = result.replace("${CLAUDE_PLUGIN_DATA}", &data_str);
    result = result.replace("$CLAUDE_PLUGIN_DATA", &data_str);

    // 替换 ${ARGUMENTS} 和 $ARGUMENTS
    result = result.replace("${ARGUMENTS}", arguments);
    result = result.replace("$ARGUMENTS", arguments);

    result
}

/// 替换变量并增加环境变量白名单替换
///
/// 在 resolve_hook_variables 基础上，额外支持环境变量展开。
/// 仅白名单内的环境变量会被替换，白名单外的保持原样。
pub fn resolve_hook_variables_with_env(
    input: &str,
    plugin_root: &Path,
    plugin_data_dir: &Path,
    arguments: &str,
    allowed_env_vars: &HashSet<String>,
) -> String {
    // 先完成插件路径和 ARGUMENTS 替换
    let intermediate = resolve_hook_variables(input, plugin_root, plugin_data_dir, arguments);

    // 使用 shellexpand 进行 env var 展开，白名单限制
    let allowed = allowed_env_vars.clone();
    match shellexpand::env_with_context::<_, String, _, std::convert::Infallible>(
        &intermediate,
        |var| {
            if allowed.contains(var) {
                Ok(Some(std::env::var(var).unwrap_or_default()))
            } else {
                Ok(None)
            }
        },
    ) {
        Ok(resolved) => resolved.to_string(),
        Err(_) => intermediate, // 展开失败时返回中间结果
    }
}

/// 将路径转换为 POSIX 格式（Windows 上 \ → /）
fn path_to_posix(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    {
        s.replace('\\', "/")
    }
    #[cfg(not(target_os = "windows"))]
    {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn plugin_root() -> PathBuf {
        PathBuf::from("/tmp/plugin")
    }

    fn plugin_data() -> PathBuf {
        PathBuf::from("/tmp/data")
    }

    #[test]
    fn test_basic_plugin_root_replacement() {
        let result = resolve_hook_variables(
            "echo ${CLAUDE_PLUGIN_ROOT}",
            &plugin_root(),
            &plugin_data(),
            "",
        );
        assert_eq!(result, "echo /tmp/plugin");
    }

    #[test]
    fn test_dollar_plugin_root_replacement() {
        let result = resolve_hook_variables(
            "echo $CLAUDE_PLUGIN_ROOT",
            &plugin_root(),
            &plugin_data(),
            "",
        );
        assert_eq!(result, "echo /tmp/plugin");
    }

    #[test]
    fn test_multi_variable_replacement() {
        let result = resolve_hook_variables(
            "${CLAUDE_PLUGIN_ROOT}/${CLAUDE_PLUGIN_DATA}",
            &plugin_root(),
            &plugin_data(),
            "",
        );
        assert_eq!(result, "/tmp/plugin//tmp/data");
    }

    #[test]
    fn test_arguments_replacement() {
        let result = resolve_hook_variables(
            "prompt: $ARGUMENTS",
            &plugin_root(),
            &plugin_data(),
            r#"{"tool":"Bash"}"#,
        );
        assert_eq!(result, r#"prompt: {"tool":"Bash"}"#);
    }

    #[test]
    fn test_arguments_brace_replacement() {
        let result = resolve_hook_variables(
            "prompt: ${ARGUMENTS}",
            &plugin_root(),
            &plugin_data(),
            r#"{"tool":"Bash"}"#,
        );
        assert_eq!(result, r#"prompt: {"tool":"Bash"}"#);
    }

    #[test]
    fn test_empty_input() {
        let result = resolve_hook_variables("", &plugin_root(), &plugin_data(), "");
        assert_eq!(result, "");
    }

    #[test]
    fn test_no_variables() {
        let input = "bash -c 'echo hello'";
        let result = resolve_hook_variables(input, &plugin_root(), &plugin_data(), "");
        assert_eq!(result, input);
    }

    #[test]
    fn test_windows_path_format() {
        // On non-Windows, just verify the path is passed through
        let root = PathBuf::from("/tmp/plugin");
        let result = resolve_hook_variables("${CLAUDE_PLUGIN_ROOT}", &root, &plugin_data(), "");
        assert_eq!(result, "/tmp/plugin");
    }

    // === env var tests ===

    #[test]
    fn test_env_var_allowed() {
        std::env::set_var("TEST_HOOK_API_KEY_FOR_TEST", "sk-xxx");
        let allowed: HashSet<String> = ["TEST_HOOK_API_KEY_FOR_TEST".to_string()]
            .into_iter()
            .collect();
        let result = resolve_hook_variables_with_env(
            "Token: ${TEST_HOOK_API_KEY_FOR_TEST}",
            &plugin_root(),
            &plugin_data(),
            "",
            &allowed,
        );
        assert_eq!(result, "Token: sk-xxx");
        std::env::remove_var("TEST_HOOK_API_KEY_FOR_TEST");
    }

    #[test]
    fn test_env_var_not_allowed() {
        let allowed: HashSet<String> = ["API_KEY".to_string()].into_iter().collect();
        let result = resolve_hook_variables_with_env(
            "${SECRET_KEY}",
            &plugin_root(),
            &plugin_data(),
            "",
            &allowed,
        );
        // shellexpand will fail to expand, returns original string
        assert_eq!(result, "${SECRET_KEY}");
    }

    #[test]
    fn test_mixed_replacement() {
        std::env::set_var("TEST_HOOK_HOME_FOR_TEST", "/home/user");
        let allowed: HashSet<String> = ["TEST_HOOK_HOME_FOR_TEST".to_string()]
            .into_iter()
            .collect();
        let result = resolve_hook_variables_with_env(
            "${CLAUDE_PLUGIN_ROOT}/${TEST_HOOK_HOME_FOR_TEST}",
            &plugin_root(),
            &plugin_data(),
            "",
            &allowed,
        );
        assert_eq!(result, "/tmp/plugin//home/user");
        std::env::remove_var("TEST_HOOK_HOME_FOR_TEST");
    }

    #[test]
    fn test_undefined_env_var() {
        let allowed: HashSet<String> = ["UNDEFINED_HOOK_TEST_VAR".to_string()]
            .into_iter()
            .collect();
        let result = resolve_hook_variables_with_env(
            "$UNDEFINED_HOOK_TEST_VAR",
            &plugin_root(),
            &plugin_data(),
            "",
            &allowed,
        );
        // shellexpand resolves to empty string for undefined vars
        assert_eq!(result, "");
    }
}
