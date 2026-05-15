    #[test]
    fn test_tips_contains_slash_command_hint() {
        let has_slash = TIPS.iter().any(|t| t.contains("/ 输入命令"));
        assert!(has_slash, "tips 应包含 '/ 输入命令' 提示");
    }

    #[test]
    fn test_tips_tab_hint() {
        let has_tab = TIPS.iter().any(|t| t.contains("Tab 补全"));
        assert!(has_tab, "tips 应包含 'Tab 补全'");
    }

    #[test]
    fn test_tips_only_reference_existing_commands() {
        // tips 中引用的 /xxx 命令必须存在于 command registry
        let existing_commands = [
            "login", "model", "history", "clear", "help", "compact", "cron", "loop", "plugin",
        ];
        for tip in TIPS {
            // 提取 tip 中的 /xxx 命令引用
            for word in tip.split_whitespace() {
                if word.starts_with('/')
                    && word.len() > 1
                    && word.chars().nth(1).is_some_and(|c| c.is_alphabetic())
                {
                    let cmd_name: String = word[1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                        .collect();
                    if !cmd_name.is_empty() {
                        assert!(
                            existing_commands.contains(&cmd_name.as_str()),
                            "tip 引用了不存在的命令 /{}: {}",
                            cmd_name,
                            tip
                        );
                    }
                }
            }
        }
    }
