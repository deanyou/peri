    struct MockBaseModel {
        id: &'static str,
        window: u32,
    }
    #[async_trait::async_trait]
    impl super::super::BaseModel for MockBaseModel {
        async fn invoke(
            &self,
            _: super::super::types::LlmRequest,
        ) -> crate::error::AgentResult<super::super::types::LlmResponse> {
            unimplemented!()
        }
        fn provider_name(&self) -> &str {
            "mock"
        }
        fn model_id(&self) -> &str {
            self.id
        }
        fn context_window(&self) -> u32 {
            self.window
        }
    }

    #[test]
    fn test_context_window_delegates_to_model() {
        let llm = BaseModelReactLLM::new(Box::new(MockBaseModel {
            id: "any-model",
            window: 128_000,
        }));
        assert_eq!(llm.context_window(), 128_000);
    }

    #[test]
    fn test_context_window_default_from_trait() {
        let llm = BaseModelReactLLM::new(Box::new(MockBaseModel {
            id: "unknown",
            window: 200_000,
        }));
        assert_eq!(llm.context_window(), 200_000);
    }
