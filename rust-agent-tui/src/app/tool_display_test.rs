    #[test]
    fn test_format_tool_name_new_names() {
        assert_eq!(format_tool_name("Read"), "Read");
        assert_eq!(format_tool_name("Write"), "Write");
        assert_eq!(format_tool_name("Edit"), "Edit");
        assert_eq!(format_tool_name("Glob"), "Glob");
        assert_eq!(format_tool_name("Grep"), "Grep");
        assert_eq!(format_tool_name("Bash"), "Shell");
        assert_eq!(format_tool_name("TodoWrite"), "Todo");
        assert_eq!(format_tool_name("AskUserQuestion"), "Ask");
        assert_eq!(format_tool_name("Agent"), "Agent");
    }

    #[test]
    fn test_format_tool_args_grep_uses_pattern() {
        let input = serde_json::json!({"pattern": "needle", "output_mode": "content"});
        let result = format_tool_args("Grep", &input, None);
        assert!(result.is_some(), "Grep 工具应返回 pattern 摘要");
        assert!(result.unwrap().contains("needle"), "应包含 pattern 内容");
    }

    #[test]
    fn test_format_tool_args_bash_uses_command() {
        let input = serde_json::json!({"command": "cargo test"});
        let result = format_tool_args("Bash", &input, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("cargo test"));
    }

    #[test]
    fn test_old_tool_names_not_matched() {
        // 验证旧工具名不再被匹配（fallback 到 to_pascal）
        assert_eq!(format_tool_name("bash"), "Bash"); // fallback
        assert_eq!(format_tool_name("read_file"), "ReadFile"); // fallback to_pascal
        assert_eq!(format_tool_name("write_file"), "WriteFile"); // fallback to_pascal
        assert_eq!(format_tool_name("search_files_rg"), "SearchFilesRg"); // fallback to_pascal
        assert_eq!(format_tool_name("launch_agent"), "LaunchAgent"); // fallback to_pascal
    }
