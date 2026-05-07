use regex::Regex;

/// 粗粒度匹配：matcher 字段
///
/// 支持三种匹配模式：
/// - `"*"` 或空字符串 → 匹配所有
/// - `"Write|Edit"` → 管道分隔的精确匹配列表
/// - `"^Bash.*"` → 正则表达式
/// - `"Write"` → 精确匹配（仅字母数字+下划线时）
pub fn matches_matcher(matcher: &str, tool_name: &str) -> bool {
    if matcher == "*" || matcher.is_empty() {
        return true;
    }
    // 管道分隔的精确匹配
    if matcher.contains('|') {
        return matcher.split('|').any(|p| p.trim() == tool_name);
    }
    // 纯字母数字+下划线 → 精确匹配
    if matcher.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return matcher == tool_name;
    }
    // 否则按正则匹配
    Regex::new(matcher)
        .map(|re| re.is_match(tool_name))
        .unwrap_or(false)
}

/// 细粒度匹配：if 条件字段（permission rule 语法）
///
/// 语法：`"{ToolName}({pattern})"`
/// 仅适用于工具事件（PreToolUse / PostToolUse / PostToolUseFailure / PermissionRequest）
pub fn matches_if_condition(
    condition: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
) -> bool {
    // 解析 "Bash(git commit)" → tool_name="Bash", rule="git commit"
    let (cond_tool, cond_rule) = match parse_permission_rule(condition) {
        Some(parsed) => parsed,
        None => return false,
    };

    if cond_tool != tool_name {
        return false;
    }

    if cond_rule.is_empty() {
        return true;
    }

    match_tool_rule(tool_name, tool_input, &cond_rule)
}

/// 解析 permission rule 语法：`"Bash(git commit)"` → `("Bash", "git commit")`
fn parse_permission_rule(rule: &str) -> Option<(String, String)> {
    let open = rule.find('(')?;
    let close = rule.rfind(')')?;
    if close <= open {
        return None;
    }
    let tool_name = rule[..open].trim().to_string();
    let pattern = rule[open + 1..close].trim().to_string();
    Some((tool_name, pattern))
}

/// 基于 tool_input 内容做字符串包含匹配
///
/// 将 tool_input 序列化为 JSON 字符串，检查 rule 是否为子串。
/// 与 Claude Code 行为一致：简单字符串包含匹配。
fn match_tool_rule(_tool_name: &str, tool_input: &serde_json::Value, rule: &str) -> bool {
    let input_str = serde_json::to_string(tool_input).unwrap_or_default();
    input_str.contains(rule)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === matcher tests ===

    #[test]
    fn test_matcher_wildcard() {
        assert!(matches_matcher("*", "Bash"));
        assert!(matches_matcher("*", "Write"));
        assert!(matches_matcher("", "Bash"));
    }

    #[test]
    fn test_matcher_exact() {
        assert!(matches_matcher("Write", "Write"));
        assert!(!matches_matcher("Write", "Edit"));
        assert!(!matches_matcher("Bash", "bash")); // case sensitive
    }

    #[test]
    fn test_matcher_pipe_list() {
        assert!(matches_matcher("Write|Edit|Grep", "Grep"));
        assert!(matches_matcher("Write|Edit|Grep", "Write"));
        assert!(!matches_matcher("Write|Edit", "Grep"));
    }

    #[test]
    fn test_matcher_regex() {
        assert!(matches_matcher("^Bash.*", "Bash -c echo"));
        assert!(!matches_matcher("^Bash", "Write"));
        assert!(matches_matcher(".*Edit.*", "EditFile"));
    }

    #[test]
    fn test_matcher_invalid_regex() {
        assert!(!matches_matcher("[invalid", "Write")); // regex compile fails → false
    }

    // === if condition tests ===

    #[test]
    fn test_if_condition_tool_name_match() {
        assert!(matches_if_condition(
            "Bash(git)",
            "Bash",
            &json!({"command": "git commit"})
        ));
    }

    #[test]
    fn test_if_condition_tool_name_mismatch() {
        assert!(!matches_if_condition(
            "Bash(git)",
            "Write",
            &json!({"path": "file.txt"})
        ));
    }

    #[test]
    fn test_if_condition_empty_rule() {
        assert!(matches_if_condition("Bash()", "Bash", &json!({})));
    }

    #[test]
    fn test_if_condition_content_contains() {
        assert!(matches_if_condition(
            "Bash(git commit)",
            "Bash",
            &json!({"command": "git commit -m msg"})
        ));
    }

    #[test]
    fn test_if_condition_content_not_contains() {
        assert!(!matches_if_condition(
            "Bash(git)",
            "Bash",
            &json!({"command": "npm install"})
        ));
    }

    // === parse_permission_rule tests ===

    #[test]
    fn test_parse_permission_rule_valid() {
        let (tool, rule) = parse_permission_rule("Bash(git commit)").unwrap();
        assert_eq!(tool, "Bash");
        assert_eq!(rule, "git commit");
    }

    #[test]
    fn test_parse_permission_rule_empty_rule() {
        let (tool, rule) = parse_permission_rule("Write()").unwrap();
        assert_eq!(tool, "Write");
        assert_eq!(rule, "");
    }

    #[test]
    fn test_parse_permission_rule_invalid() {
        assert!(parse_permission_rule("no_parens").is_none());
        assert!(parse_permission_rule(")(invalid(").is_none());
    }
}
