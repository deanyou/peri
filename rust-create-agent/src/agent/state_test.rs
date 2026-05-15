    #[test]
    fn test_agent_state_new() {
        let state = AgentState::new("/workspace");
        assert_eq!(state.cwd(), "/workspace");
        assert_eq!(state.messages().len(), 0);
        assert_eq!(state.current_step(), 0);
    }

    #[test]
    fn test_agent_state_messages() {
        let mut state = AgentState::new("/workspace");
        state.add_message(BaseMessage::human("hello"));
        state.add_message(BaseMessage::ai("hi there"));
        assert_eq!(state.messages().len(), 2);
        assert!(matches!(state.messages()[0], BaseMessage::Human { .. }));
    }

    #[test]
    fn test_agent_state_context() {
        let state = AgentState::new("/workspace")
            .with_context("key1", "value1")
            .with_context("key2", "value2");
        assert_eq!(state.get_context("key1"), Some("value1"));
        assert_eq!(state.get_context("missing"), None);
    }

    #[test]
    fn test_token_tracker_default() {
        let state = AgentState::new("/tmp");
        assert_eq!(state.token_tracker().llm_call_count, 0);
        assert_eq!(state.token_tracker().total_input_tokens, 0);
    }

    #[test]
    fn test_token_tracker_accumulate() {
        use crate::llm::types::TokenUsage;
        let mut state = AgentState::new("/tmp");
        state.token_tracker_mut().accumulate(&TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(30),
            cache_read_input_tokens: None,
            request_id: None,
        });
        assert_eq!(state.token_tracker().total_input_tokens, 100);
        assert_eq!(state.token_tracker().llm_call_count, 1);
    }
