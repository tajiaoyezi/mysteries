pub mod compacting;
pub mod context;
pub mod message;

pub use compacting::run_compact_command;
pub use compacting::{CompactCommandOutcome, Compacting, CompactionSettings, SUMMARY_HEADER};
pub use context::{ContextError, ContextStrategy, Passthrough};

use crate::agent::message::Message;
use crate::error::{AgentError, ProviderError};
use crate::permission::{gate, PermissionDecider, PermissionDecision, PermissionMode};
use crate::provider::{DeltaSink, Depth, ModelRequest, Provider, ThinkingConfig, Usage};
use crate::tool::{PermissionLevel, ToolContext, ToolOutcome, ToolRegistry};
use std::sync::{Arc, Mutex};

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Mysteries, a helpful coding assistant. Do not claim to be Claude, ChatGPT, OpenAI, Anthropic, or any specific upstream model. If asked about your model identity, say you are running inside Mysteries and the configured model name is shown in the status line.";
pub const PLAN_MODE_INSTRUCTION: &str = "你在 plan 模式(只读:read_file/grep/glob/web_*,不改文件/不执行命令)。用户只是问 → 直接答;撞到岔路/歧义 → ask_user 弹选项让用户定;用户要执行任务 → 调研够了 submit_plan 交结构化 plan、每步带可验收 validation。每步 description 一句话简述(尽量 ≤30 字),不写长段落、不堆细节;细节留到执行时或放进 validation";
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
                thinking: None,
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

    fn on_usage(&self, _usage: &Usage) {}
}

pub struct NoopObserver;

impl AgentObserver for NoopObserver {}

pub struct Agent {
    provider: Arc<dyn Provider>,
    registry: ToolRegistry,
    decider: Box<dyn PermissionDecider>,
    model: String,
    max_iterations: u32,
    strategy: Box<dyn ContextStrategy>,
    permission_mode: Arc<Mutex<PermissionMode>>,
    thinking_depth: Arc<Mutex<Depth>>,
}

impl Agent {
    pub fn new(
        provider: Arc<dyn Provider>,
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
            strategy: Box::new(Passthrough),
            permission_mode: Arc::new(Mutex::new(PermissionMode::Normal)),
            thinking_depth: Arc::new(Mutex::new(Depth::Low)),
        }
    }

    pub fn set_thinking_depth(&mut self, depth: Arc<Mutex<Depth>>) {
        self.thinking_depth = depth;
    }

    pub fn set_permission_mode(&mut self, mode: Arc<Mutex<PermissionMode>>) {
        self.permission_mode = mode;
    }

    pub fn set_model(&mut self, model: String, history: &mut [Message]) {
        for message in history.iter_mut() {
            if let Message::Assistant { thinking, .. } = message {
                thinking.clear();
            }
        }
        self.model = model.clone();
        self.strategy.set_model(model);
    }

    pub fn set_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider.clone();
        self.strategy.set_provider(provider);
    }

    pub fn set_strategy(&mut self, strategy: Box<dyn ContextStrategy>) {
        self.strategy = strategy;
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
        let mut last_usage: Option<Usage> = None;

        for _ in 0..self.max_iterations {
            observer.on_status(AgentStatus::CallingModel);
            let depth = *self
                .thinking_depth
                .lock()
                .expect("thinking_depth mutex poisoned");
            let mode = *self
                .permission_mode
                .lock()
                .expect("permission_mode mutex poisoned");
            let mut msgs = self.strategy.prepare(history, last_usage.as_ref()).await?;
            if mode == PermissionMode::Plan {
                msgs.insert(0, Message::System(PLAN_MODE_INSTRUCTION.to_string()));
            }
            let response = self
                .provider
                .complete(
                    ModelRequest {
                        model: self.model.clone(),
                        messages: msgs,
                        tools: self.registry.schemas_for(mode),
                        max_tokens: None,
                        thinking: Some(ThinkingConfig { depth }),
                    },
                    sink,
                )
                .await?;
            let text = response.text;
            let tool_calls = response.tool_calls;
            let thinking = response.thinking;
            last_usage = response.usage;
            if let Some(ref usage) = last_usage {
                observer.on_usage(usage);
            }

            history.push(Message::Assistant {
                text: text.clone(),
                tool_calls: tool_calls.clone(),
                thinking,
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

                if mode == PermissionMode::Plan
                    && tool.permission_level() != PermissionLevel::ReadOnly
                {
                    let outcome = ToolOutcome {
                        content: "plan mode forbids non-readonly tools".to_string(),
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

                if mode != PermissionMode::Plan && tool.plan_only() {
                    let outcome = ToolOutcome {
                        content: "submit_plan is only available in plan mode".to_string(),
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

        let depth = *self
            .thinking_depth
            .lock()
            .expect("thinking_depth mutex poisoned");
        let msgs = self.strategy.prepare(history, last_usage.as_ref()).await?;
        let response = self
            .provider
            .complete(
                ModelRequest {
                    model: self.model.clone(),
                    messages: msgs,
                    tools: Vec::new(),
                    max_tokens: None,
                    thinking: Some(ThinkingConfig { depth }),
                },
                sink,
            )
            .await?;
        let text = response.text;
        let tool_calls = response.tool_calls;
        let thinking = response.thinking;

        history.push(Message::Assistant {
            text: text.clone(),
            tool_calls,
            thinking,
        });

        if text.is_empty() {
            return Err(AgentError::MaxIterations {
                limit: self.max_iterations,
            });
        }

        Ok(text)
    }
}

impl From<ContextError> for AgentError {
    fn from(err: ContextError) -> Self {
        AgentError::Context(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        run_single_turn, Agent, AgentObserver, AgentStatus, ContextStrategy, DEFAULT_SYSTEM_PROMPT,
        PLAN_MODE_INSTRUCTION,
    };
    use crate::agent::message::Message;
    use crate::error::{AgentError, ProviderError};
    use crate::permission::{PermissionDecider, PermissionDecision, PermissionMode};
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, Depth, FinishReason, ModelResponse, Provider, ThinkingBlock, ThinkingConfig,
        ToolCall, Usage,
    };
    use crate::tool::edit::WriteFileTool;
    use crate::tool::plan::{MockPlanApprover, Plan, PlanApprover, PlanDecision, SubmitPlanTool};
    use crate::tool::{PermissionLevel, Tool, ToolContext, ToolOutcome, ToolRegistry};
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq)]
    enum ObservedEvent {
        Status(AgentStatus),
        Usage(Usage),
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

        fn on_usage(&self, usage: &Usage) {
            self.events
                .lock()
                .unwrap()
                .push(ObservedEvent::Usage(usage.clone()));
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

    fn tool_response_with_thinking(
        tool_calls: Vec<ToolCall>,
        thinking: Vec<ThinkingBlock>,
    ) -> ModelResponse {
        ModelResponse {
            text: String::new(),
            tool_calls,
            finish_reason: FinishReason::ToolCalls,
            usage: None,
            thinking,
        }
    }

    fn response(text: &str) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }
    }

    fn tool_response(tool_calls: Vec<ToolCall>) -> ModelResponse {
        ModelResponse {
            text: String::new(),
            tool_calls,
            finish_reason: FinishReason::ToolCalls,
            usage: None,
            thinking: Vec::new(),
        }
    }

    fn tool_response_with_usage(tool_calls: Vec<ToolCall>, usage: Usage) -> ModelResponse {
        ModelResponse {
            text: String::new(),
            tool_calls,
            finish_reason: FinishReason::ToolCalls,
            usage: Some(usage),
            thinking: Vec::new(),
        }
    }

    fn response_with_usage(text: &str, usage: Usage) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: Some(usage),
            thinking: Vec::new(),
        }
    }

    #[derive(Default)]
    struct LastUsageRecorder {
        seen: Mutex<Vec<Option<Usage>>>,
    }

    impl LastUsageRecorder {
        fn seen(&self) -> Vec<Option<Usage>> {
            self.seen.lock().unwrap().clone()
        }
    }

    struct RecordingStrategy {
        recorder: Arc<LastUsageRecorder>,
    }

    #[derive(Default)]
    struct StrategySwitchRecorder {
        provider_names: Mutex<Vec<String>>,
        models: Mutex<Vec<String>>,
    }

    struct RecordingSwitchStrategy {
        recorder: Arc<StrategySwitchRecorder>,
    }

    struct NamedProvider(&'static str);

    #[async_trait]
    impl Provider for NamedProvider {
        fn name(&self) -> &str {
            self.0
        }

        async fn complete(
            &self,
            _req: crate::provider::ModelRequest,
            _sink: &dyn DeltaSink,
        ) -> Result<ModelResponse, ProviderError> {
            Ok(ModelResponse {
                text: String::new(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl ContextStrategy for RecordingSwitchStrategy {
        async fn prepare(
            &self,
            history: &[Message],
            _last_usage: Option<&Usage>,
        ) -> Result<Vec<Message>, crate::agent::ContextError> {
            Ok(history.to_vec())
        }

        fn set_provider(&mut self, provider: Arc<dyn Provider>) {
            self.recorder
                .provider_names
                .lock()
                .unwrap()
                .push(provider.name().to_string());
        }

        fn set_model(&mut self, model: String) {
            self.recorder.models.lock().unwrap().push(model);
        }
    }

    #[async_trait]
    impl ContextStrategy for RecordingStrategy {
        async fn prepare(
            &self,
            history: &[Message],
            last_usage: Option<&Usage>,
        ) -> Result<Vec<Message>, crate::agent::ContextError> {
            self.recorder.seen.lock().unwrap().push(last_usage.cloned());
            Ok(history.to_vec())
        }
    }

    #[tokio::test]
    async fn agent_passes_previous_response_usage_as_last_usage_on_next_prepare() {
        let first_usage = Usage {
            input_tokens: 11,
            output_tokens: 22,
        };
        let provider = Arc::new(MockProvider::new(vec![
            tool_response_with_usage(
                vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: json!({ "input": "from tool" }),
                }],
                first_usage.clone(),
            ),
            response_with_usage(
                "done",
                Usage {
                    input_tokens: 33,
                    output_tokens: 44,
                },
            ),
        ]));
        let recorder = Arc::new(LastUsageRecorder::default());
        let mut agent = Agent::new(
            provider,
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_strategy(Box::new(RecordingStrategy {
            recorder: recorder.clone(),
        }));
        let sink = NoopSink;
        let mut history = vec![
            Message::System("system".to_string()),
            Message::User("hello".to_string()),
        ];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();
        assert_eq!(text, "done");

        let seen = recorder.seen();
        assert_eq!(seen.len(), 2, "two model rounds => two prepare calls");
        assert_eq!(seen[0], None, "first prepare should receive None");
        assert_eq!(
            seen[1],
            Some(first_usage),
            "second prepare should receive previous response usage"
        );
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
            PermissionLevel::Execute
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
            provider.clone(),
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
                thinking: Vec::new(),
            })
        );
        assert_eq!(provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn set_model_updates_next_model_request() {
        let provider = Arc::new(MockProvider::new(vec![response("after switch")]));
        let mut agent = Agent::new(
            provider.clone(),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        agent.set_model("m2".to_string(), &mut history);
        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "after switch");
        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].model, "m2");
    }

    #[tokio::test]
    async fn set_provider_and_set_model_propagate_to_context_strategy() {
        let initial_provider = Arc::new(NamedProvider("initial"));
        let new_provider = Arc::new(NamedProvider("switched"));
        let recorder = Arc::new(StrategySwitchRecorder::default());
        let mut agent = Agent::new(
            initial_provider,
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        agent.set_strategy(Box::new(RecordingSwitchStrategy {
            recorder: recorder.clone(),
        }));

        agent.set_provider(new_provider);
        agent.set_model("m2".to_string(), &mut []);

        assert_eq!(
            recorder.provider_names.lock().unwrap().as_slice(),
            &["switched"],
            "set_provider should propagate to context strategy"
        );
        assert_eq!(
            recorder.models.lock().unwrap().as_slice(),
            &["m2"],
            "set_model should propagate to context strategy"
        );
    }

    #[tokio::test]
    async fn set_provider_routes_next_run_to_new_provider() {
        let old_provider = Arc::new(MockProvider::new(vec![response("old")]));
        let new_provider = Arc::new(MockProvider::new(vec![response("new")]));
        let mut agent = Agent::new(
            old_provider.clone(),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        agent.set_provider(new_provider.clone());
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "new");
        assert!(old_provider.recorded_requests().is_empty());
        assert_eq!(new_provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn set_provider_on_passthrough_strategy_is_no_op() {
        let old_provider = Arc::new(MockProvider::new(vec![response("ok")]));
        let new_provider = Arc::new(MockProvider::new(vec![response("ok")]));
        let mut agent = Agent::new(
            old_provider.clone(),
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        agent.set_provider(new_provider.clone());
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];
        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "ok");
        assert_eq!(new_provider.recorded_requests().len(), 1);
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
            provider.clone(),
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
                thinking: Vec::new(),
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
                thinking: Vec::new(),
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
            provider,
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
            provider,
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
    async fn run_observed_emits_on_usage_when_model_response_has_usage() {
        let first_usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
        };
        let provider = Arc::new(MockProvider::new(vec![
            tool_response_with_usage(
                vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: json!({ "input": "from tool" }),
                }],
                first_usage.clone(),
            ),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let mut history = vec![Message::User("hello".to_string())];

        agent
            .run_observed(&mut history, &ctx(), &sink, &observer)
            .await
            .unwrap();

        let usage_events = observer
            .events()
            .into_iter()
            .filter_map(|event| match event {
                ObservedEvent::Usage(usage) => Some(usage),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(usage_events, vec![first_usage]);
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
            provider,
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
            provider,
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
            provider,
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
            provider,
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
            provider.clone(),
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
                thinking: Vec::new(),
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
            provider,
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
            provider,
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
            provider,
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

    struct FlippingPlanApprover {
        mode: Arc<Mutex<PermissionMode>>,
    }

    #[async_trait]
    impl PlanApprover for FlippingPlanApprover {
        async fn approve(&self, _plan: &Plan) -> PlanDecision {
            *self.mode.lock().expect("permission_mode mutex poisoned") =
                PermissionMode::AcceptEdits;
            PlanDecision::Approve
        }
    }

    struct CountingPlanApprover {
        calls: Arc<AtomicUsize>,
        decision: PlanDecision,
    }

    #[async_trait]
    impl PlanApprover for CountingPlanApprover {
        async fn approve(&self, _plan: &Plan) -> PlanDecision {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.decision.clone()
        }
    }

    fn registry_with_plan_tools(approver: Box<dyn PlanApprover>) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool)).unwrap();
        registry.register(Box::new(WriteFileTool)).unwrap();
        registry
            .register(Box::new(SubmitPlanTool::new(approver)))
            .unwrap();
        registry
    }

    fn sample_plan_call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "submit_plan".to_string(),
            arguments: json!({
                "title": "Test plan",
                "steps": [
                    {
                        "description": "Do work",
                        "validation": "tests pass"
                    }
                ]
            }),
        }
    }

    #[tokio::test]
    async fn agent_plan_mode_exposes_readonly_and_plan_only_schemas_only() {
        let mode = Arc::new(Mutex::new(PermissionMode::Plan));
        let provider = Arc::new(MockProvider::new(vec![response("planning")]));
        let mut agent = Agent::new(
            provider.clone(),
            registry_with_plan_tools(Box::new(MockPlanApprover::new(PlanDecision::Approve))),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(mode);
        let sink = NoopSink;
        let mut history = vec![Message::User("plan this".to_string())];

        agent.run(&mut history, &ctx(), &sink).await.unwrap();

        let recorded = provider.recorded_requests();
        let tool_names = recorded[0]
            .tools
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(tool_names, vec!["echo", "submit_plan"]);
    }

    #[tokio::test]
    async fn agent_plan_mode_injects_transient_instruction_not_history() {
        let mode = Arc::new(Mutex::new(PermissionMode::Plan));
        let provider = Arc::new(MockProvider::new(vec![response("planning")]));
        let mut agent = Agent::new(
            provider.clone(),
            registry_with_plan_tools(Box::new(MockPlanApprover::new(PlanDecision::Approve))),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(mode);
        let sink = NoopSink;
        let mut history = vec![
            Message::System("base".to_string()),
            Message::User("plan this".to_string()),
        ];

        agent.run(&mut history, &ctx(), &sink).await.unwrap();

        let request_msgs = &provider.recorded_requests()[0].messages;
        assert_eq!(
            request_msgs[0],
            Message::System(PLAN_MODE_INSTRUCTION.to_string())
        );
        assert_eq!(request_msgs[1], Message::System("base".to_string()));
        assert_eq!(history[0], Message::System("base".to_string()));
    }

    #[tokio::test]
    async fn agent_normal_mode_does_not_inject_plan_instruction() {
        let provider = Arc::new(MockProvider::new(vec![response("ok")]));
        let agent = Agent::new(
            provider.clone(),
            registry_with_plan_tools(Box::new(MockPlanApprover::new(PlanDecision::Approve))),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![
            Message::System("base".to_string()),
            Message::User("hello".to_string()),
        ];

        agent.run(&mut history, &ctx(), &sink).await.unwrap();

        let request_msgs = &provider.recorded_requests()[0].messages;
        assert!(!request_msgs.iter().any(|message| {
            matches!(message, Message::System(text) if text == PLAN_MODE_INSTRUCTION)
        }));
        assert_eq!(request_msgs, &history[..2]);
    }

    #[tokio::test]
    async fn agent_plan_mode_snapshot_blocks_edit_after_submit_plan_flips_mode() {
        let mode = Arc::new(Mutex::new(PermissionMode::Plan));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                sample_plan_call("call-plan"),
                ToolCall {
                    id: "call-write".to_string(),
                    name: "write_file".to_string(),
                    arguments: json!({ "path": "blocked.txt", "content": "nope" }),
                },
            ]),
            response("done"),
        ]));
        let temp = tempfile::tempdir().unwrap();
        let mut agent = Agent::new(
            provider,
            registry_with_plan_tools(Box::new(FlippingPlanApprover { mode: mode.clone() })),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(mode.clone());
        let sink = NoopSink;
        let mut history = vec![Message::User("execute".to_string())];
        let tool_ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let text = agent.run(&mut history, &tool_ctx, &sink).await.unwrap();

        assert_eq!(text, "done");
        assert_eq!(*mode.lock().unwrap(), PermissionMode::AcceptEdits);
        assert!(!temp.path().join("blocked.txt").exists());
        assert_eq!(
            history[2],
            Message::ToolResult {
                call_id: "call-plan".to_string(),
                content: "计划已批准,按上述 plan 逐步执行;每开始一步先 update_plan 标记 in_progress、每完成一步 update_plan 标记 done 并附 validation 自检结果".to_string(),
                is_error: false,
            }
        );
        assert!(matches!(
            &history[3],
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "call-write"
        ));
    }

    #[tokio::test]
    async fn agent_normal_mode_rejects_plan_only_tools_without_executing() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![sample_plan_call("call-plan")]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry_with_plan_tools(Box::new(CountingPlanApprover {
                calls: calls.clone(),
                decision: PlanDecision::Approve,
            })),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("submit".to_string())];

        agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &history[2],
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "call-plan"
        ));
    }

    #[tokio::test]
    async fn run_observed_forced_final_request_carries_current_thinking_depth() {
        let depth = Arc::new(Mutex::new(Depth::High));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "call-1".to_string(),
                name: "echo".to_string(),
                arguments: json!({ "input": "loop" }),
            }]),
            response("forced final"),
        ]));
        let mut agent = Agent::new(
            provider.clone(),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            1,
        );
        agent.set_thinking_depth(depth);
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "forced final");
        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded[0].thinking,
            Some(ThinkingConfig { depth: Depth::High })
        );
        assert_eq!(
            recorded[1].thinking,
            Some(ThinkingConfig { depth: Depth::High }),
            "forced-final must re-read depth snapshot outside the loop"
        );
    }

    #[tokio::test]
    async fn run_observed_round_trips_assistant_thinking_in_history() {
        let thinking = vec![ThinkingBlock {
            text: "plan".to_string(),
            signature: Some("sig-abc".to_string()),
            redacted: false,
        }];
        let provider = Arc::new(MockProvider::new(vec![
            tool_response_with_thinking(
                vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: json!({ "input": "first" }),
                }],
                thinking.clone(),
            ),
            response("done"),
        ]));
        let agent = Agent::new(
            provider.clone(),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];

        let text = agent.run(&mut history, &ctx(), &sink).await.unwrap();

        assert_eq!(text, "done");
        assert_eq!(
            history[1],
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: json!({ "input": "first" }),
                }],
                thinking: thinking.clone(),
            }
        );
        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        assert!(recorded[1].messages.iter().any(|msg| {
            matches!(
                msg,
                Message::Assistant {
                    thinking: roundtrip,
                    ..
                } if *roundtrip == thinking
            )
        }));
    }

    #[test]
    fn set_model_strips_assistant_thinking_from_history() {
        let provider = Arc::new(NamedProvider("mock"));
        let mut agent = Agent::new(
            provider,
            ToolRegistry::new(),
            Box::new(AllowAll),
            "m1".to_string(),
            4,
        );
        let mut history = vec![Message::Assistant {
            text: "done".to_string(),
            tool_calls: Vec::new(),
            thinking: vec![ThinkingBlock {
                text: "secret".to_string(),
                signature: Some("sig-cross-model".to_string()),
                redacted: false,
            }],
        }];

        agent.set_model("m2".to_string(), &mut history);

        assert_eq!(
            history,
            vec![Message::Assistant {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                thinking: Vec::new(),
            }]
        );
    }
}
