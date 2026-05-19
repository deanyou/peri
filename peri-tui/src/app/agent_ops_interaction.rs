use super::*;
use peri_acp::transport::types::RequestId;
use peri_middlewares::hitl::BatchItem;

impl App {
    /// Handle ACP RequestPermission: create HITL approval dialog.
    pub(crate) fn handle_acp_request_permission(
        &mut self,
        id: RequestId,
        params: serde_json::Value,
    ) -> (bool, bool, bool) {
        use agent_client_protocol::schema::RequestPermissionRequest;
        use tokio::sync::oneshot;

        let req = match serde_json::from_value::<RequestPermissionRequest>(params) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse RequestPermissionRequest");
                return (false, false, false);
            }
        };

        let tool_name = req
            .tool_call
            .fields
            .title
            .unwrap_or_else(|| "unknown".to_string());
        let tool_input = req
            .tool_call
            .fields
            .raw_input
            .unwrap_or(serde_json::Value::Null);

        let batch_items = vec![BatchItem {
            tool_name,
            input: tool_input,
        }];

        // Create oneshot bridge — the confirm() handler will call bridge_tx.send(decisions)
        let (bridge_tx, _bridge_rx) = oneshot::channel::<Vec<HitlDecision>>();

        // Store ACP request id for response dispatch in hitl_ops.rs
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pending_acp_request_id = Some(id);

        let prompt = HitlBatchPrompt::new(batch_items, bridge_tx);
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .interaction_prompt = Some(InteractionPrompt::Approval(prompt));

        (true, true, false) // pause event consumption, wait for user confirmation
    }

    /// Handle ACP elicitation/create: create AskUser dialog.
    pub(crate) fn handle_acp_elicitation(
        &mut self,
        id: RequestId,
        params: serde_json::Value,
    ) -> (bool, bool, bool) {
        use agent_client_protocol_schema::{CreateElicitationRequest, ElicitationMode};
        use peri_middlewares::ask_user::{AskUserBatchRequest, AskUserOption, AskUserQuestionData};
        use tokio::sync::oneshot;

        let req = match serde_json::from_value::<CreateElicitationRequest>(params) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to parse CreateElicitationRequest");
                return (false, false, false);
            }
        };

        let mut questions = Vec::new();

        if let ElicitationMode::Form(form) = req.mode {
            for (prop_id, prop) in &form.requested_schema.properties {
                let (title, description, is_multi, options) = match prop {
                    agent_client_protocol_schema::ElicitationPropertySchema::String(s) => (
                        s.title.clone(),
                        s.description.clone(),
                        false,
                        s.one_of
                            .as_ref()
                            .map(|opts| {
                                opts.iter()
                                    .map(|o| AskUserOption {
                                        label: o.title.clone(),
                                        description: None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    ),
                    agent_client_protocol_schema::ElicitationPropertySchema::Array(a) => (
                        a.title.clone(),
                        a.description.clone(),
                        true,
                        match &a.items {
                            agent_client_protocol_schema::MultiSelectItems::Titled(t) => t
                                .options
                                .iter()
                                .map(|o| AskUserOption {
                                    label: o.title.clone(),
                                    description: None,
                                })
                                .collect(),
                            _ => vec![],
                        },
                    ),
                    _ => continue,
                };
                questions.push(AskUserQuestionData {
                    tool_call_id: prop_id.clone(),
                    question: description.unwrap_or_default(),
                    header: title.unwrap_or_default(),
                    multi_select: is_multi,
                    options,
                });
            }
        }

        // Create oneshot bridge — confirm() handler will call bridge_tx.send(answers)
        let (bridge_tx, _bridge_rx) = oneshot::channel::<Vec<String>>();

        // Store ACP request id for response dispatch in ask_user_ops.rs
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pending_acp_request_id = Some(id);
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .pending_ask_user = Some(false);

        let (batch_req, _) = AskUserBatchRequest::new(questions);
        let batch_req_bridged = AskUserBatchRequest {
            questions: batch_req.questions,
            response_tx: bridge_tx,
        };
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .interaction_prompt = Some(InteractionPrompt::Questions(
            AskUserBatchPrompt::from_request(batch_req_bridged),
        ));

        (true, true, false) // pause event consumption, wait for user input
    }

    /// Handle AgentEvent::InteractionRequest: create Approval or Questions dialog.
    pub(crate) fn handle_interaction_request(
        &mut self,
        ctx: peri_agent::interaction::InteractionContext,
        response_tx: tokio::sync::oneshot::Sender<peri_agent::interaction::InteractionResponse>,
    ) -> (bool, bool, bool) {
        use peri_agent::interaction::{
            ApprovalDecision, InteractionContext, InteractionResponse, QuestionAnswer,
        };
        use peri_middlewares::ask_user::{AskUserBatchRequest, AskUserOption, AskUserQuestionData};
        use tokio::sync::oneshot;

        match ctx {
            InteractionContext::Approval { items } => {
                let batch_items: Vec<BatchItem> = items
                    .iter()
                    .map(|i| BatchItem {
                        tool_name: i.tool_name.clone(),
                        input: i.tool_input.clone(),
                    })
                    .collect();
                let (bridge_tx, bridge_rx) = oneshot::channel::<Vec<HitlDecision>>();
                tokio::spawn(async move {
                    if let Ok(decisions) = bridge_rx.await {
                        let approval_decisions: Vec<ApprovalDecision> = decisions
                            .into_iter()
                            .map(|d| match d {
                                HitlDecision::Approve => ApprovalDecision::Approve,
                                HitlDecision::Reject => ApprovalDecision::Reject {
                                    reason: "User rejected".to_string(),
                                },
                                HitlDecision::Edit(v) => ApprovalDecision::Edit { new_input: v },
                                HitlDecision::Respond(msg) => {
                                    ApprovalDecision::Respond { message: msg }
                                }
                            })
                            .collect();
                        let _ =
                            response_tx.send(InteractionResponse::Decisions(approval_decisions));
                    }
                });
                self.session_mgr.sessions[self.session_mgr.active]
                    .agent
                    .interaction_prompt = Some(InteractionPrompt::Approval(HitlBatchPrompt::new(
                    batch_items,
                    bridge_tx,
                )));
                (true, true, false) // 暂停消费，等待用户确认
            }
            InteractionContext::Questions { requests } => {
                let ask_questions: Vec<AskUserQuestionData> = requests
                    .iter()
                    .map(|q| AskUserQuestionData {
                        tool_call_id: q.id.clone(),
                        question: q.question.clone(),
                        header: q.header.clone(),
                        multi_select: q.multi_select,
                        options: q
                            .options
                            .iter()
                            .map(|o| AskUserOption {
                                label: o.label.clone(),
                                description: o.description.clone(),
                            })
                            .collect(),
                    })
                    .collect();
                let (bridge_tx, bridge_rx) = oneshot::channel::<Vec<String>>();
                let ids: Vec<String> = requests.iter().map(|q| q.id.clone()).collect();
                tokio::spawn(async move {
                    if let Ok(answers) = bridge_rx.await {
                        let question_answers: Vec<QuestionAnswer> = ids
                            .into_iter()
                            .zip(answers)
                            .map(|(id, answer)| QuestionAnswer {
                                id,
                                selected: vec![answer.clone()],
                                text: Some(answer),
                            })
                            .collect();
                        let _ = response_tx.send(InteractionResponse::Answers(question_answers));
                    }
                });
                self.session_mgr.sessions[self.session_mgr.active]
                    .agent
                    .pending_ask_user = Some(false);
                let (batch_req, _) = AskUserBatchRequest::new(ask_questions);
                let batch_req_bridged = AskUserBatchRequest {
                    questions: batch_req.questions,
                    response_tx: bridge_tx,
                };
                self.session_mgr.sessions[self.session_mgr.active]
                    .agent
                    .interaction_prompt = Some(InteractionPrompt::Questions(
                    AskUserBatchPrompt::from_request(batch_req_bridged),
                ));
                (true, true, false) // 暂停消费，等待用户输入
            }
        }
    }
}
