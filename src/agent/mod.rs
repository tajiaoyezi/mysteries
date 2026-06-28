pub mod message;

use crate::agent::message::Message;
use crate::error::{AgentError, ProviderError};
use crate::permission::{gate, PermissionDecider, PermissionDecision};
use crate::provider::{DeltaSink, ModelRequest, Provider};
use crate::tool::{PermissionLevel, ToolContext, ToolOutcome, ToolRegistry};

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Mysteries, a helpful coding assistant. Do not claim to be Claude, ChatGPT, OpenAI, Anthropic, or any specific upstream model. If asked about your model identity, say you are running inside Mysteries and the configured model name is shown in the status line.";
const DEFAULT_MODEL: &str = "mock-model";

pub async fn run_single_turn(
    provider: &dyn Provider,
    prompt: &str,
    sink: &dyn DeltaSink,
) -> Result<String, ProviderError> {
    let response = provider
        .complete(
            ModelRequest {
                model: DEFAULT_MODEL.to_string(),
                messages: vec![
                    Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                    Message::User(prompt.to_string()),
                ],
                tools: Vec::new(),
                max_tokens: None,
            },
            sink,
        )
        .await?;

    Ok(response.text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    CallingModel,
    ExecutingTool(String),
    WaitingForPermission,
}

pub trait AgentObserver: Send + Sync {
    fn on_status(&self, _status: AgentStatus) {}

    fn on_tool_call_started(
        &self,
        _id: &str,
        _name: &str,
        _args: &serde_json::Value,
        _readonly: bool,
    ) {
    }

    fn on_tool_call_finished(&self, _id: &str, _outcome: &crate::tool::ToolOutcome) {}
}

pub struct NoopObserver;

impl AgentObserver for NoopObserver {}

pub struct Agent {
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    decider: Box<dyn PermissionDecider>,
    model: String,
    max_iterations: u32,
}

impl Agent {
    pub fn new(
        provider: Box<dyn Provider>,
        registry: ToolRegistry,
        decider: Box<dyn PermissionDecider>,
        model: String,
        max_iterations: u32,
    ) -> Self {
        Self {
            provider,
            registry,
            decider,
            model,
            max_iterations,
        }
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    pub async fn run(
        &self,
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        sink: &dyn DeltaSink,
    ) -> Result<String, AgentError> {
        self.run_observed(history, ctx, sink, &NoopObserver).await
    }

    pub async fn run_observed(
        &self,
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        sink: &dyn DeltaSink,
        observer: &dyn AgentObserver,
    ) -> Result<String, AgentError> {
        for _ in 0..self.max_iterations {
            observer.on_status(AgentStatus::CallingModel);
            let response = self
                .provider
                .complete(
                    ModelRequest {
                        model: self.model.clone(),
                        messages: history.clone(),
                        tools: self.registry.schemas(),
                        max_tokens: None,
                    },
                    sink,
                )
                .await?;
            let text = response.text;
            let tool_calls = response.tool_calls;

            history.push(Message::Assistant {
                text: text.clone(),
                tool_calls: tool_calls.clone(),
            });

            if tool_calls.is_empty() {
                observer.on_status(AgentStatus::Idle);
                return Ok(text);
            }

            for call in tool_calls {
                let Some(tool) = self.registry.get(&call.name) else {
                    let outcome = ToolOutcome {
                        content: format!("unknown tool: {}", call.name),
                        is_error: true,
                        truncated: false,
                        exit: None,
                    };
                    history.push(Message::ToolResult {
                        call_id: call.id.clone(),
                        content: outcome.content.clone(),
                        is_error: outcome.is_error,
                    });
                    observer.on_tool_call_finished(&call.id, &outcome);
                    continue;
                };

                let readonly = tool.permission_level() == PermissionLevel::ReadOnly;
                observer.on_tool_call_started(&call.id, &call.name, &call.arguments, readonly);

                if !readonly {
                    observer.on_status(AgentStatus::WaitingForPermission);
                }

                if gate(&call, tool, self.decider.as_ref()).await == PermissionDecision::Deny {
                    let outcome = ToolOutcome {
                        content: "user denied tool execution".to_string(),
                        is_error: true,
                        truncated: false,
                        exit: None,
                    };
                    history.push(Message::ToolResult {
                        call_id: call.id.clone(),
                        content: outcome.content.clone(),
                        is_error: outcome.is_error,
                    });
                    observer.on_tool_call_finished(&call.id, &outcome);
                    continue;
                }

                observer.on_status(AgentStatus::ExecutingTool(call.name.clone()));
                let outcome = tool.execute(call.arguments.clone(), ctx).await;

                history.push(Message::ToolResult {
                    call_id: call.id.clone(),
                    content: outcome.content.clone(),
                    is_error: outcome.is_error,
                });
                observer.on_tool_call_finished(&call.id, &outcome);
            }
        }

        let response = self
            .provider
            .complete(
                ModelRequest {
                    model: self.model.clone(),
                    messages: history.clone(),
                    tools: Vec::new(),
                    max_tokens: None,
                },
                sink,
            )
            .await?;
        let text = response.text;
        let tool_calls = response.tool_calls;

        history.push(Message::Assistant {
            text: text.clone(),
            tool_calls,
        });

        if text.is_empty() {
            return Err(AgentError::MaxIterations {
                limit: self.max_iterations,
            });
        }

        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{run_single_turn, Agent, AgentObserver, AgentStatus, DEFAULT_SYSTEM_PROMPT};
    use crate::agent::message::Message;
    use crate::error::{AgentError, ProviderError};
    use crate::permission::{PermissionDecider, PermissionDecision};
    use crate::provider::mock::MockProvider;
    use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
    use crate::tool::edit::WriteFileTool;
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome, ToolRegistry};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq)]
    enum ObservedEvent {
        Status(AgentStatus),
        ToolCallStarted {
            id: String,
            name: String,
            args: Value,
            readonly: bool,
        },
        ToolCallFinished {
            id: String,
            outcome: ToolOutcome,
        },
    }

    #[derive(Default)]
    struct RecordingObserver {
        events: Mutex<Vec<ObservedEvent>>,
    }

    impl RecordingObserver {
        fn events(&self) -> Vec<ObservedEvent> {
            self.events.lock().unwrap().drain(..).collect()
        }
    }

    impl AgentObserver for RecordingObserver {
        fn on_status(&self, status: AgentStatus) {
            self.events
                .lock()
                .unwrap()
                .push(ObservedEvent::Status(status));
        }

        fn on_tool_call_started(&self, id: &str, name: &str, args: &Value, readonly: bool) {
            self.events
                .lock()
                .unwrap()
                .push(ObservedEvent::ToolCallStarted {
                    id: id.to_string(),
                    name: name.to_string(),
                    args: args.clone(),
                    readonly,
                });
        }

        fn on_tool_call_finished(&self, id: &str, outcome: &ToolOutcome) {
            self.events
                .lock()
                .unwrap()
                .push(ObservedEvent::ToolCallFinished {
                    id: id.to_string(),
                    outcome: outcome.clone(),
                });
        }
    }

    struct CaptureSink {
        chunks: Mutex<Vec<String>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
            }
        }
    }

    impl DeltaSink for CaptureSink {
        fn on_text(&self, text: &str) {
            self.chunks.lock().unwrap().push(text.to_string());
        }
    }

    fn response(text: &str) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }
    }

    fn tool_response(tool_calls: Vec<ToolCall>) -> ModelResponse {
        ModelResponse {
            text: String::new(),
            tool_calls,
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        }
    }

    #[test]
    fn default_system_prompt_constrains_model_identity_claims() {
        assert!(DEFAULT_SYSTEM_PROMPT.contains("Do not claim to be Claude"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("ChatGPT"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("OpenAI"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("Anthropic"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("configured model name is shown in the status line"));
    }

    struct NoopSink;

    impl DeltaSink for NoopSink {
        fn on_text(&self, _text: &str) {}
    }

    struct AllowAll;

    #[async_trait]
    impl PermissionDecider for AllowAll {
        async fn decide(&self, _call: &ToolCall, _tool: &dyn Tool) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct DenyAll;

    #[async_trait]
    impl PermissionDecider for DenyAll {
        async fn decide(&self, _call: &ToolCall, _tool: &dyn Tool) -> PermissionDecision {
            PermissionDecision::Deny
        }
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo an input string"
        }

        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"]
            })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::ReadOnly
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: args["input"].as_str().unwrap().to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    struct ErrorTool;

    #[async_trait]
    impl Tool for ErrorTool {
        fn name(&self) -> &str {
            "fail"
        }

        fn description(&self) -> &str {
            "Return an error outcome"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::ReadOnly
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            ToolOutcome {
                content: "tool failed".to_string(),
                is_error: true,
                truncated: false,
                exit: None,
            }
        }
    }

    struct ConfirmTool {
        executions: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for ConfirmTool {
        fn name(&self) -> &str {
            "confirm"
        }

        fn description(&self) -> &str {
            "Requires confirmation"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::RequiresConfirmation
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            self.executions.fetch_add(1, Ordering::SeqCst);
            ToolOutcome {
                content: "changed".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes: 4096,
        }
    }

    fn registry_with_echo() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry
    }

    fn registry_with_error_tool() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(ErrorTool)).unwrap();
        registry
    }

    fn registry_with_confirm_tool(executions: Arc<AtomicUsize>) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(ConfirmTool { executions }))
            .unwrap();
        registry
    }

    fn registry_with_write_file() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(WriteFileTool)).unwrap();
        registry
    }

    #[tokio::test]
    async fn run_single_turn_builds_request_returns_text_and_streams_delta() {
        let provider = MockProvider::new(vec![response("model reply")]);
        let sink = CaptureSink::new();

        let text = run_single_turn(&provider, "user prompt", &sink)
            .await
            .unwrap();

        assert_eq!(text, "model reply");
        assert_eq!(*sink.chunks.lock().unwrap(), vec!["model reply"]);

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].messages.len(), 2);
        assert!(matches!(recorded[0].messages[0], Message::System(_)));
        assert_eq!(
            recorded[0].messages[1],
            Message::User("user prompt".to_string())
        );
    }

    #[tokio::test]
    async fn agent_loop_stops_after_text_response_and_records_assistant() {
        let provider = Arc::new(MockProvider::new(vec![response("final reply")]));
        let agent = Agent::new(
            Box::new(provider.clone()),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![
            Message::System("system".to_string()),
            Message::User("hello".to_string()),
        ];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "final reply");
        assert_eq!(
            history.last(),
            Some(&Message::Assistant {
                text: "final reply".to_string(),
                tool_calls: Vec::new(),
            })
        );
        assert_eq!(provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn set_model_updates_next_model_request() {
        let provider = Arc::new(MockProvider::new(vec![response("after switch")]));
        let mut agent = Agent::new(
            Box::new(provider.clone()),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        agent.set_model("m2".to_string());
        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "after switch");
        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].model, "m2");
    }

    #[tokio::test]
    async fn agent_loop_executes_tool_records_result_and_sends_accumulated_history() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "from tool" }),
            }]),
            response("done"),
        ]));
        let agent = Agent::new(
            Box::new(provider.clone()),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![
            Message::System("system".to_string()),
            Message::User("hello".to_string()),
        ];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "done");
        assert_eq!(history.len(), 5);
        assert_eq!(
            history[2],
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: json!({ "input": "from tool" }),
                }],
            }
        );
        assert_eq!(
            history[3],
            Message::ToolResult {
                call_id: "call-1".to_string(),
                content: "from tool".to_string(),
                is_error: false,
            }
        );
        assert_eq!(
            history[4],
            Message::Assistant {
                text: "done".to_string(),
                tool_calls: Vec::new(),
            }
        );

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[1].messages, history[..4].to_vec());
        assert_eq!(recorded[1].tools.len(), 1);
        assert_eq!(recorded[1].tools[0].name, "echo");
    }

    #[tokio::test]
    async fn run_observed_emits_tool_call_sequence() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "from tool" }),
            }]),
            response("done"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent
            .run_observed(&mut history, &ctx(), &sink, &observer)
            .await
            .unwrap();

        assert_eq!(text, "done");
        assert_eq!(
            observer.events(),
            vec![
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::ToolCallStarted {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    args: json!({ "input": "from tool" }),
                    readonly: true,
                },
                ObservedEvent::Status(AgentStatus::ExecutingTool("echo".to_string())),
                ObservedEvent::ToolCallFinished {
                    id: "call-1".to_string(),
                    outcome: ToolOutcome {
                        content: "from tool".to_string(),
                        is_error: false,
                        truncated: false,
                        exit: None,
                    },
                },
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::Status(AgentStatus::Idle),
            ]
        );
    }

    #[tokio::test]
    async fn run_observed_emits_denied_permission_sequence() {
        let executions = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "confirm".to_string(),
                arguments: json!({}),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_confirm_tool(executions.clone()),
            Box::new(DenyAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent
            .run_observed(&mut history, &ctx(), &sink, &observer)
            .await
            .unwrap();

        assert_eq!(text, "recovered");
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert_eq!(
            observer.events(),
            vec![
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::ToolCallStarted {
                    id: "call-1".to_string(),
                    name: "confirm".to_string(),
                    args: json!({}),
                    readonly: false,
                },
                ObservedEvent::Status(AgentStatus::WaitingForPermission),
                ObservedEvent::ToolCallFinished {
                    id: "call-1".to_string(),
                    outcome: ToolOutcome {
                        content: "user denied tool execution".to_string(),
                        is_error: true,
                        truncated: false,
                        exit: None,
                    },
                },
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::Status(AgentStatus::Idle),
            ]
        );
    }

    #[tokio::test]
    async fn agent_loop_continues_after_tool_error_result() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "fail".to_string(),
                arguments: json!({}),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_error_tool(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "recovered");
        assert_eq!(
            history[2],
            Message::ToolResult {
                call_id: "call-1".to_string(),
                content: "tool failed".to_string(),
                is_error: true,
            }
        );
    }

    #[tokio::test]
    async fn agent_loop_continues_after_unknown_tool() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "missing".to_string(),
                arguments: json!({}),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "recovered");
        assert!(matches!(
            &history[2],
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "call-1"
        ));
    }

    #[tokio::test]
    async fn agent_loop_records_denied_tool_without_executing_it() {
        let executions = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "confirm".to_string(),
                arguments: json!({}),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_confirm_tool(executions.clone()),
            Box::new(DenyAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "recovered");
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &history[2],
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "call-1"
        ));
    }

    #[tokio::test]
    async fn agent_loop_denies_write_file_without_creating_file() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "write_file".to_string(),
                arguments: json!({ "path": "denied.txt", "content": "blocked" }),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_write_file(),
            Box::new(DenyAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let text = agent.run(&mut history, &ctx, &sink).await.unwrap();

        assert_eq!(text, "recovered");
        assert!(!temp.path().join("denied.txt").exists());
        assert!(matches!(
            &history[2],
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "call-1"
        ));
    }

    #[tokio::test]
    async fn agent_loop_forces_final_text_with_tools_disabled_after_iteration_limit() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "first" }),
            }]),
            tool_response(vec![ToolCall {
                id: "call-2".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "second" }),
            }]),
            response("forced final"),
        ]));
        let agent = Agent::new(
            Box::new(provider.clone()),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            2,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "forced final");
        assert_eq!(
            history.last(),
            Some(&Message::Assistant {
                text: "forced final".to_string(),
                tool_calls: Vec::new(),
            })
        );

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0].tools.len(), 1);
        assert_eq!(recorded[1].tools.len(), 1);
        assert!(recorded[2].tools.is_empty());
    }

    #[tokio::test]
    async fn agent_loop_returns_max_iterations_when_forced_final_text_is_empty() {
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "again" }),
            }]),
            response(""),
        ]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            1,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let err = agent.run(&mut history, &ctx(), &sink).await.unwrap_err();

        assert_eq!(err, AgentError::MaxIterations { limit: 1 });
    }

    #[tokio::test]
    async fn agent_loop_returns_provider_error_when_forced_final_call_fails() {
        let provider = Arc::new(MockProvider::new(vec![tool_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "echo".to_string(),
            arguments: json!({ "input": "again" }),
        }])]));
        let agent = Agent::new(
            Box::new(provider),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            1,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let err = agent.run(&mut history, &ctx(), &sink).await.unwrap_err();

        assert!(matches!(
            err,
            AgentError::Provider(ProviderError::Transport(_))
        ));
    }

    #[tokio::test]
    async fn agent_loop_returns_provider_error_as_fatal() {
        let provider = Arc::new(MockProvider::new(Vec::new()));
        let agent = Agent::new(
            Box::new(provider),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let err = agent.run(&mut history, &ctx(), &sink).await.unwrap_err();

        assert!(matches!(
            err,
            AgentError::Provider(ProviderError::Transport(_))
        ));
    }
}
