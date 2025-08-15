use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ReviewDecision;
use mcp_types::JSONRPCErrorError;
use mcp_types::RequestId;
use tokio::sync::oneshot;
use tracing::error;
use uuid::Uuid;

use crate::error_code::INTERNAL_ERROR_CODE;
use crate::error_code::INVALID_REQUEST_ERROR_CODE;
use crate::json_to_toml::json_to_toml;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::OutgoingNotification;
use crate::wire_format::APPLY_PATCH_APPROVAL_METHOD;
use crate::wire_format::AddConversationListenerParams;
use crate::wire_format::AddConversationSubscriptionResponse;
use crate::wire_format::ApplyPatchApprovalParams;
use crate::wire_format::ApplyPatchApprovalResponse;
use crate::wire_format::ClientRequest;
use crate::wire_format::ConversationId;
use crate::wire_format::EXEC_COMMAND_APPROVAL_METHOD;
use crate::wire_format::ExecCommandApprovalParams;
use crate::wire_format::ExecCommandApprovalResponse;
use crate::wire_format::InputItem as WireInputItem;
use crate::wire_format::InterruptConversationParams;
use crate::wire_format::InterruptConversationResponse;
use crate::wire_format::NewConversationParams;
use crate::wire_format::NewConversationResponse;
use crate::wire_format::RemoveConversationListenerParams;
use crate::wire_format::RemoveConversationSubscriptionResponse;
use crate::wire_format::SendUserMessageParams;
use crate::wire_format::SendUserMessageResponse;
use crate::wire_format::SendUserTurnParams;
use crate::wire_format::SendUserTurnResponse;
use codex_core::protocol::InputItem as CoreInputItem;
use codex_core::protocol::Op;

/// Handles JSON-RPC messages for Codex conversations.
pub(crate) struct CodexMessageProcessor {
    conversation_manager: Arc<ConversationManager>,
    outgoing: Arc<OutgoingMessageSender>,
    codex_linux_sandbox_exe: Option<PathBuf>,
    conversation_listeners: HashMap<Uuid, oneshot::Sender<()>>,
}

impl CodexMessageProcessor {
    pub fn new(
        conversation_manager: Arc<ConversationManager>,
        outgoing: Arc<OutgoingMessageSender>,
        codex_linux_sandbox_exe: Option<PathBuf>,
    ) -> Self {
        Self {
            conversation_manager,
            outgoing,
            codex_linux_sandbox_exe,
            conversation_listeners: HashMap::new(),
        }
    }

    pub async fn process_request(&mut self, request: ClientRequest) {
        match request {
            ClientRequest::NewConversation { request_id, params } => {
                // Do not tokio::spawn() to process new_conversation()
                // asynchronously because we need to ensure the conversation is
                // created before processing any subsequent messages.
                self.process_new_conversation(request_id, params).await;
            }
            ClientRequest::SendUserMessage { request_id, params } => {
                self.send_user_message(request_id, params).await;
            }
            ClientRequest::SendUserTurn { request_id, params } => {
                self.send_user_turn(request_id, params).await;
            }
            ClientRequest::InterruptConversation { request_id, params } => {
                self.interrupt_conversation(request_id, params).await;
            }
            ClientRequest::AddConversationListener { request_id, params } => {
                self.add_conversation_listener(request_id, params).await;
            }
            ClientRequest::RemoveConversationListener { request_id, params } => {
                self.remove_conversation_listener(request_id, params).await;
            }
        }
    }

    async fn process_new_conversation(&self, request_id: RequestId, params: NewConversationParams) {
        let config = match derive_config_from_params(params, self.codex_linux_sandbox_exe.clone()) {
            Ok(config) => config,
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("error deriving config: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
                return;
            }
        };

        match self.conversation_manager.new_conversation(config).await {
            Ok(conversation_id) => {
                let NewConversation {
                    conversation_id,
                    session_configured,
                    ..
                } = conversation_id;
                let response = NewConversationResponse {
                    conversation_id: ConversationId(conversation_id),
                    model: session_configured.model,
                };
                self.outgoing.send_response(request_id, response).await;
            }
            Err(err) => {
                let error = JSONRPCErrorError {
                    code: INTERNAL_ERROR_CODE,
                    message: format!("error creating conversation: {err}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }

    async fn send_user_message(&self, request_id: RequestId, params: SendUserMessageParams) {
        let SendUserMessageParams {
            conversation_id,
            items,
        } = params;
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id.0)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        // Submit user input to the conversation.
        let _ = conversation
            .submit(Op::UserInput {
                items: mapped_items,
            })
            .await;

        // Acknowledge with an empty result.
        self.outgoing
            .send_response(request_id, SendUserMessageResponse {})
            .await;
    }

    async fn send_user_turn(&self, request_id: RequestId, params: SendUserTurnParams) {
        let SendUserTurnParams {
            conversation_id,
            items,
            cwd,
            approval_policy,
            sandbox_policy,
            model,
            effort,
            summary,
        } = params;

        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id.0)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let mapped_items: Vec<CoreInputItem> = items
            .into_iter()
            .map(|item| match item {
                WireInputItem::Text { text } => CoreInputItem::Text { text },
                WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
                WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
            })
            .collect();

        let _ = conversation
            .submit(Op::UserTurn {
                items: mapped_items,
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                effort,
                summary,
            })
            .await;

        self.outgoing
            .send_response(request_id, SendUserTurnResponse {})
            .await;
    }

    async fn interrupt_conversation(
        &mut self,
        request_id: RequestId,
        params: InterruptConversationParams,
    ) {
        let InterruptConversationParams { conversation_id } = params;
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id.0)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {conversation_id}"),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let _ = conversation.submit(Op::Interrupt).await;

        // Apparently CodexConversation does not send an ack for Op::Interrupt,
        // so we can reply to the request right away.
        self.outgoing
            .send_response(request_id, InterruptConversationResponse {})
            .await;
    }

    async fn add_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: AddConversationListenerParams,
    ) {
        let AddConversationListenerParams { conversation_id } = params;
        let Ok(conversation) = self
            .conversation_manager
            .get_conversation(conversation_id.0)
            .await
        else {
            let error = JSONRPCErrorError {
                code: INVALID_REQUEST_ERROR_CODE,
                message: format!("conversation not found: {}", conversation_id.0),
                data: None,
            };
            self.outgoing.send_error(request_id, error).await;
            return;
        };

        let subscription_id = Uuid::new_v4();
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.conversation_listeners
            .insert(subscription_id, cancel_tx);
        let outgoing_for_task = self.outgoing.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => {
                        // User has unsubscribed, so exit this task.
                        break;
                    }
                    event = conversation.next_event() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(err) => {
                                tracing::warn!("conversation.next_event() failed with: {err}");
                                break;
                            }
                        };

                        // For now, we send a notification for every event,
                        // JSON-serializing the `Event` as-is, but we will move
                        // to creating a special enum for notifications with a
                        // stable wire format.
                        let method = format!("codex/event/{}", event.msg);
                        let mut params = match serde_json::to_value(event.clone()) {
                            Ok(serde_json::Value::Object(map)) => map,
                            Ok(_) => {
                                tracing::error!("event did not serialize to an object");
                                continue;
                            }
                            Err(err) => {
                                tracing::error!("failed to serialize event: {err}");
                                continue;
                            }
                        };
                        params.insert("conversationId".to_string(), conversation_id.to_string().into());

                        outgoing_for_task.send_notification(OutgoingNotification {
                            method,
                            params: Some(params.into()),
                        })
                        .await;

                        apply_bespoke_event_handling(event, conversation_id, conversation.clone(), outgoing_for_task.clone()).await;
                    }
                }
            }
        });
        let response = AddConversationSubscriptionResponse { subscription_id };
        self.outgoing.send_response(request_id, response).await;
    }

    async fn remove_conversation_listener(
        &mut self,
        request_id: RequestId,
        params: RemoveConversationListenerParams,
    ) {
        let RemoveConversationListenerParams { subscription_id } = params;
        match self.conversation_listeners.remove(&subscription_id) {
            Some(sender) => {
                // Signal the spawned task to exit and acknowledge.
                let _ = sender.send(());
                let response = RemoveConversationSubscriptionResponse {};
                self.outgoing.send_response(request_id, response).await;
            }
            None => {
                let error = JSONRPCErrorError {
                    code: INVALID_REQUEST_ERROR_CODE,
                    message: format!("subscription not found: {subscription_id}"),
                    data: None,
                };
                self.outgoing.send_error(request_id, error).await;
            }
        }
    }
}

async fn apply_bespoke_event_handling(
    event: Event,
    conversation_id: ConversationId,
    conversation: Arc<CodexConversation>,
    outgoing: Arc<OutgoingMessageSender>,
) {
    let Event { id: event_id, msg } = event;
    match msg {
        EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id,
            changes,
            reason,
            grant_root,
        }) => {
            let params = ApplyPatchApprovalParams {
                conversation_id,
                call_id,
                file_changes: changes,
                reason,
                grant_root,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(APPLY_PATCH_APPROVAL_METHOD, Some(value))
                .await;
            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            tokio::spawn(async move {
                on_patch_approval_response(event_id, rx, conversation).await;
            });
        }
        EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
            call_id,
            command,
            cwd,
            reason,
        }) => {
            let params = ExecCommandApprovalParams {
                conversation_id,
                call_id,
                command,
                cwd,
                reason,
            };
            let value = serde_json::to_value(&params).unwrap_or_default();
            let rx = outgoing
                .send_request(EXEC_COMMAND_APPROVAL_METHOD, Some(value))
                .await;

            // TODO(mbolin): Enforce a timeout so this task does not live indefinitely?
            tokio::spawn(async move {
                on_exec_approval_response(event_id, rx, conversation).await;
            });
        }
        _ => {}
    }
}

fn derive_config_from_params(
    params: NewConversationParams,
    codex_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<Config> {
    let NewConversationParams {
        model,
        profile,
        cwd,
        approval_policy,
        sandbox,
        config: cli_overrides,
        base_instructions,
        include_plan_tool,
        include_apply_patch_tool,
    } = params;
    let overrides = ConfigOverrides {
        model,
        config_profile: profile,
        cwd: cwd.map(PathBuf::from),
        approval_policy: approval_policy.map(Into::into),
        sandbox_mode: sandbox.map(Into::into),
        model_provider: None,
        codex_linux_sandbox_exe,
        base_instructions,
        include_plan_tool,
        include_apply_patch_tool,
        disable_response_storage: None,
        show_raw_agent_reasoning: None,
    };

    let cli_overrides = cli_overrides
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, json_to_toml(v)))
        .collect();

    Config::load_with_cli_overrides(cli_overrides, overrides)
}

async fn on_patch_approval_response(
    event_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    codex: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            error!("request failed: {err:?}");
            if let Err(submit_err) = codex
                .submit(Op::PatchApproval {
                    id: event_id.clone(),
                    decision: ReviewDecision::Denied,
                })
                .await
            {
                error!("failed to submit denied PatchApproval after request failure: {submit_err}");
            }
            return;
        }
    };

    let response =
        serde_json::from_value::<ApplyPatchApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ApplyPatchApprovalResponse: {err}");
            ApplyPatchApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = codex
        .submit(Op::PatchApproval {
            id: event_id,
            decision: response.decision,
        })
        .await
    {
        error!("failed to submit PatchApproval: {err}");
    }
}

async fn on_exec_approval_response(
    event_id: String,
    receiver: tokio::sync::oneshot::Receiver<mcp_types::Result>,
    conversation: Arc<CodexConversation>,
) {
    let response = receiver.await;
    let value = match response {
        Ok(value) => value,
        Err(err) => {
            tracing::error!("request failed: {err:?}");
            return;
        }
    };

    // Try to deserialize `value` and then make the appropriate call to `codex`.
    let response =
        serde_json::from_value::<ExecCommandApprovalResponse>(value).unwrap_or_else(|err| {
            error!("failed to deserialize ExecCommandApprovalResponse: {err}");
            // If we cannot deserialize the response, we deny the request to be
            // conservative.
            ExecCommandApprovalResponse {
                decision: ReviewDecision::Denied,
            }
        });

    if let Err(err) = conversation
        .submit(Op::ExecApproval {
            id: event_id,
            decision: response.decision,
        })
        .await
    {
        error!("failed to submit ExecApproval: {err}");
    }
}
