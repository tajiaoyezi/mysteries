pub mod message;

use crate::agent::message::Message;
use crate::error::{AgentError, ProviderError};
use crate::permission::{gate, PermissionDecider, PermissionDecision};
use crate::provider::{DeltaSink, ModelRequest, Provider};
use crate::tool::{ToolContext, ToolRegistry};

const DEFAULT_SYSTEM_PROMPT: &str = "You are Mysteries, a helpful coding assistant.";
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

    pub async fn run(
        &self,
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        sink: &dyn DeltaSink,
    ) -> Result<String, AgentError> {
        for _ in 0..self.max_iterations {
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
                return Ok(text);
            }

            for call in tool_calls {
                let Some(tool) = self.registry.get(&call.name) else {
                    history.push(Message::ToolResult {
                        call_id: call.id,
                        content: format!("unknown tool: {}", call.name),
                        is_error: true,
                    });
                    continue;
                };

                if gate(&call, tool, self.decider.as_ref()).await == PermissionDecision::Deny {
                    history.push(Message::ToolResult {
                        call_id: call.id,
                        content: "user denied tool execution".to_string(),
                        is_error: true,
                    });
                    continue;
                }

                let outcome = tool.execute(call.arguments.clone(), ctx).await;

                history.push(Message::ToolResult {
                    call_id: call.id,
                    content: outcome.content,
                    is_error: outcome.is_error,
                });
            }
        }

        Err(AgentError::MaxIterations {
            limit: self.max_iterations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{run_single_turn, Agent};
    use crate::agent::message::Message;
    use crate::error::{AgentError, ProviderError};
    use crate::permission::{PermissionDecider, PermissionDecision};
    use crate::provider::mock::MockProvider;
    use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome, ToolRegistry};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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
        }
    }

    fn tool_response(tool_calls: Vec<ToolCall>) -> ModelResponse {
        ModelResponse {
            text: String::new(),
            tool_calls,
            finish_reason: FinishReason::ToolCalls,
        }
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
        registry.register(Box::new(EchoTool));
        registry
    }

    fn registry_with_error_tool() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(ErrorTool));
        registry
    }

    fn registry_with_confirm_tool(executions: Arc<AtomicUsize>) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(ConfirmTool { executions }));
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
    async fn agent_loop_returns_max_iterations_when_limit_is_hit() {
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

        assert_eq!(err, AgentError::MaxIterations { limit: 1 });
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
