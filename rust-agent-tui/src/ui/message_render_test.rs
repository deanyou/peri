    fn make_agent(id: &str, task: &str, tools: usize, error: bool) -> AgentSummary {
        AgentSummary {
            agent_id: id.to_string(),
            task_preview: task.to_string(),
            tool_count: tools,
            is_error: error,
            final_result: if error {
                Some("failed".to_string())
            } else {
                Some("done".to_string())
            },
        }
    }

    #[test]
    fn test_render_batch_summary_collapsed() {
        let agents = vec![
            make_agent("agent-1", "task one", 3, false),
            make_agent("agent-2", "task two", 5, false),
            make_agent("agent-3", "task three", 0, false),
        ];
        let lines = render_batch_summary(&agents, &true);
        // Header + 3 行 agent 摘要 = 4 行
        assert_eq!(lines.len(), 4, "折叠态应有 header + 3 行摘要");
        // Header 应包含 "3 agents finished"
        let header_text: String = lines[0].spans.iter().map(|s| s.content.clone()).collect();
        assert!(
            header_text.contains("3 agents finished"),
            "header 应显示 agent 数量: {}",
            header_text
        );
    }

    #[test]
    fn test_render_batch_summary_expanded() {
        let agents = vec![
            make_agent("agent-1", "task one", 3, false),
            make_agent("agent-2", "task two", 5, false),
        ];
        let lines = render_batch_summary(&agents, &false);
        // Header + 2 * (task_preview + final_result) = 5 行
        assert_eq!(lines.len(), 5, "展开态应有 header + 2*(task+result)");
    }

    #[test]
    fn test_render_batch_summary_with_error() {
        let agents = vec![
            make_agent("agent-1", "task one", 3, false),
            make_agent("agent-2", "task two", 1, true),
            make_agent("agent-3", "task three", 2, true),
        ];
        let lines = render_batch_summary(&agents, &true);
        let header_text: String = lines[0].spans.iter().map(|s| s.content.clone()).collect();
        assert!(
            header_text.contains("2 failed"),
            "header 应显示失败数: {}",
            header_text
        );
    }

    #[test]
    fn test_render_batch_summary_tree_connectors() {
        let agents = vec![
            make_agent("agent-1", "task one", 3, false),
            make_agent("agent-2", "task two", 5, false),
            make_agent("agent-3", "task three", 0, false),
        ];
        let lines = render_batch_summary(&agents, &true);
        // 第一个 agent 应使用 ├─
        let line1_text: String = lines[1].spans.iter().map(|s| s.content.clone()).collect();
        assert!(
            line1_text.contains("├─"),
            "非最后一个 agent 应使用 ├─: {}",
            line1_text
        );
        // 最后一个 agent 应使用 └─
        let line3_text: String = lines[3].spans.iter().map(|s| s.content.clone()).collect();
        assert!(
            line3_text.contains("└─"),
            "最后一个 agent 应使用 └─: {}",
            line3_text
        );
    }

    #[test]
    fn test_render_single_agent_unchanged() {
        // batch_agents 为空时走现有渲染路径，不经过 render_batch_summary
        // 此测试验证 render_batch_summary 对空 agents 列表的边界行为
        let agents: Vec<AgentSummary> = vec![];
        let lines = render_batch_summary(&agents, &true);
        assert_eq!(lines.len(), 1, "空 agents 应只有 header");
        let header_text: String = lines[0].spans.iter().map(|s| s.content.clone()).collect();
        assert!(
            header_text.contains("0 agents"),
            "header 应包含 0 agents: {}",
            header_text
        );
    }
