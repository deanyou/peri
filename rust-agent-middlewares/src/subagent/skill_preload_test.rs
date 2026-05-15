    fn write_skill(dir: &std::path::Path, name: &str, desc: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nSkill content for {}.\n",
            name, desc, name, name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn test_no_op_when_empty_names() {
        // Arrange
        let dir = tempdir().unwrap();
        let mw = SkillPreloadMiddleware::new(vec![], dir.path().to_str().unwrap());
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        assert_eq!(state.messages().len(), 0);
    }

    #[tokio::test]
    async fn test_inject_single_skill() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "api-guide", "API 开发指南");

        let mw = SkillPreloadMiddleware::new(
            vec!["api-guide".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: Ai + Tool = 2 条消息
        assert_eq!(state.messages().len(), 2, "应注入 2 条消息（Ai + Tool）");
        assert!(
            matches!(&state.messages()[0], BaseMessage::Ai { .. }),
            "第一条应为 Ai"
        );
        assert!(
            matches!(&state.messages()[1], BaseMessage::Tool { .. }),
            "第二条应为 Tool"
        );
    }

    #[tokio::test]
    async fn test_inject_multiple_skills() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "skill-a", "技能 A");
        write_skill(&skills_dir, "skill-b", "技能 B");
        write_skill(&skills_dir, "skill-c", "技能 C");

        let mw = SkillPreloadMiddleware::new(
            vec![
                "skill-a".to_string(),
                "skill-b".to_string(),
                "skill-c".to_string(),
            ],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: Ai + Tool × 3 = 4 条消息
        assert_eq!(state.messages().len(), 4, "3 个 skill 应注入 4 条消息");
    }

    #[tokio::test]
    async fn test_skip_missing_skill() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "exists", "存在的 skill");

        let mw = SkillPreloadMiddleware::new(
            vec!["exists".to_string(), "nonexistent".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: 只有 "exists" → Ai + Tool = 2 条
        assert_eq!(state.messages().len(), 2, "不存在的 skill 应静默跳过");
    }

    #[tokio::test]
    async fn test_no_op_when_all_skills_missing() {
        // Arrange
        let dir = tempdir().unwrap();
        let mw = SkillPreloadMiddleware::new(
            vec!["nonexistent".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        assert_eq!(state.messages().len(), 0, "全部找不到时应 no-op");
    }

    #[tokio::test]
    async fn test_message_order() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "skill-x", "技能 X");
        write_skill(&skills_dir, "skill-y", "技能 Y");

        let mw = SkillPreloadMiddleware::new(
            vec!["skill-x".to_string(), "skill-y".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let msgs = state.messages();
        assert!(
            matches!(&msgs[0], BaseMessage::Ai { .. }),
            "messages[0] 应为 Ai"
        );
        assert!(msgs[0].has_tool_calls(), "Ai 消息应包含工具调用");
        assert_eq!(msgs[0].tool_calls().len(), 2, "Ai 消息应有 2 个工具调用");
        assert!(
            matches!(&msgs[1], BaseMessage::Tool { .. }),
            "messages[1] 应为 Tool"
        );
        assert!(
            matches!(&msgs[2], BaseMessage::Tool { .. }),
            "messages[2] 应为 Tool"
        );
    }

    #[tokio::test]
    async fn test_tool_call_ids_match() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "my-skill", "My skill");

        let mw =
            SkillPreloadMiddleware::new(vec!["my-skill".to_string()], dir.path().to_str().unwrap());
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let msgs = state.messages();
        let ai_id = &msgs[0].tool_calls()[0].id;
        if let BaseMessage::Tool { tool_call_id, .. } = &msgs[1] {
            assert_eq!(
                tool_call_id, ai_id,
                "Tool 消息的 tool_call_id 应与 Ai 消息一致"
            );
        } else {
            unreachable!("messages[1] 应为 Tool");
        }
    }

    #[tokio::test]
    async fn test_tool_result_contains_skill_content() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "commit-skill", "提交技能");

        let mw = SkillPreloadMiddleware::new(
            vec!["commit-skill".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let tool_content = state.messages()[1].content();
        assert!(
            tool_content.contains("Skill content for commit-skill"),
            "Tool 结果应包含 skill 全文内容"
        );
    }
