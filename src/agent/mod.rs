pub mod compacting;
pub mod context;
pub mod message;

pub use compacting::run_compact_command;
pub use compacting::{CompactCommandOutcome, Compacting, CompactionSettings, SUMMARY_HEADER};
pub use context::{ContextError, ContextStrategy, Passthrough};

use crate::agent::message::Message;
use crate::error::{AgentError, ProviderError};
use crate::permission::{
    gate, PermissionDecider, PermissionDenial, PermissionGateOutcome, PermissionMode,
};
use crate::provider::{DeltaSink, Depth, ModelRequest, Provider, ThinkingConfig, ToolCall, Usage};
use crate::tool::{PermissionLevel, ToolConcurrency, ToolContext, ToolOutcome, ToolRegistry};
use std::sync::{Arc, Mutex};

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Mysteries, a helpful coding assistant. Do not claim to be Claude, ChatGPT, OpenAI, Anthropic, or any specific upstream model. If asked about your model identity, say you are running inside Mysteries and the configured model name is shown in the status line.";
pub const PLAN_MODE_INSTRUCTION: &str = "你在 plan 模式。可使用 ReadOnly 与 Network 工具调研；每次 Network 调用仍须用户授权。禁止 Edit 与 Execute 工具。用户只是问 → 直接答;撞到岔路/歧义 → ask_user 弹选项让用户定;用户要执行任务 → 调研够了 submit_plan 交结构化 plan、每步带可验收 validation。每步 description 一句话简述(尽量 ≤30 字),不写长段落、不堆细节;细节留到执行时或放进 validation";
const DEFAULT_MODEL: &str = "mock-model";
/// 同一 Agent 并行安全批次的最大同时 in-flight execute 数。
pub const MAX_PARALLEL_TOOL_CALLS: usize = 4;

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
    /// 并行安全批次（段长 >1）的聚合状态；count 为整段已调度 occurrence 总数。
    ExecutingTools(usize),
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

    /// 交互式模型切换：清空 history 中全部 Assistant.thinking。
    pub fn set_model(&mut self, model: String, history: &mut [Message]) {
        for message in history.iter_mut() {
            if let Message::Assistant { thinking, .. } = message {
                thinking.clear();
            }
        }
        self.restore_model(model);
    }

    /// Session 恢复等路径：只更新 model / ContextStrategy，不碰 thinking。
    pub fn restore_model(&mut self, model: String) {
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

            self.dispatch_tool_calls(&tool_calls, mode, history, ctx, observer)
                .await;
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

    /// 工具存在 + ParallelSafe + ReadOnly + !plan_only 才可进入并行段（host clamp）。
    fn is_parallel_eligible(&self, call: &ToolCall) -> bool {
        let Some(tool) = self.registry.get(&call.name) else {
            return false;
        };
        tool.concurrency() == ToolConcurrency::ParallelSafe
            && tool.permission_level() == PermissionLevel::ReadOnly
            && !tool.plan_only()
    }

    async fn dispatch_tool_calls(
        &self,
        tool_calls: &[ToolCall],
        mode: PermissionMode,
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        observer: &dyn AgentObserver,
    ) {
        let mut index = 0;
        while index < tool_calls.len() {
            if self.is_parallel_eligible(&tool_calls[index]) {
                let mut end = index + 1;
                while end < tool_calls.len() && self.is_parallel_eligible(&tool_calls[end]) {
                    end += 1;
                }
                let batch = &tool_calls[index..end];
                if batch.len() == 1 {
                    self.execute_tool_call_serial(&batch[0], mode, history, ctx, observer)
                        .await;
                } else {
                    self.execute_parallel_safe_batch(batch, history, ctx, observer)
                        .await;
                }
                index = end;
            } else {
                self.execute_tool_call_serial(&tool_calls[index], mode, history, ctx, observer)
                    .await;
                index += 1;
            }
        }
    }

    fn publish_tool_outcome(
        history: &mut Vec<Message>,
        observer: &dyn AgentObserver,
        call_id: &str,
        outcome: ToolOutcome,
    ) {
        history.push(Message::ToolResult {
            call_id: call_id.to_string(),
            content: outcome.content.clone(),
            is_error: outcome.is_error,
        });
        observer.on_tool_call_finished(call_id, &outcome);
    }

    async fn execute_tool_call_serial(
        &self,
        call: &ToolCall,
        mode: PermissionMode,
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        observer: &dyn AgentObserver,
    ) {
        let Some(tool) = self.registry.get(&call.name) else {
            let outcome = ToolOutcome {
                content: format!("unknown tool: {}", call.name),
                is_error: true,
                truncated: false,
                exit: None,
            };
            Self::publish_tool_outcome(history, observer, &call.id, outcome);
            return;
        };

        let readonly = tool.permission_level() == PermissionLevel::ReadOnly;
        observer.on_tool_call_started(&call.id, &call.name, &call.arguments, readonly);

        if mode == PermissionMode::Plan
            && matches!(
                tool.permission_level(),
                PermissionLevel::Edit | PermissionLevel::Execute
            )
        {
            let outcome = ToolOutcome {
                content: "plan mode forbids non-readonly tools".to_string(),
                is_error: true,
                truncated: false,
                exit: None,
            };
            Self::publish_tool_outcome(history, observer, &call.id, outcome);
            return;
        }

        if mode != PermissionMode::Plan && tool.plan_only() {
            let outcome = ToolOutcome {
                content: "submit_plan is only available in plan mode".to_string(),
                is_error: true,
                truncated: false,
                exit: None,
            };
            Self::publish_tool_outcome(history, observer, &call.id, outcome);
            return;
        }

        if !readonly {
            observer.on_status(AgentStatus::WaitingForPermission);
        }

        let gate_outcome = gate(call, tool, self.decider.as_ref()).await;
        if let PermissionGateOutcome::Deny(denial) = gate_outcome {
            let outcome = ToolOutcome {
                content: match denial {
                    PermissionDenial::UserDenied => "user denied tool execution".to_string(),
                    PermissionDenial::NetworkUnauthorizable(reason) => reason,
                },
                is_error: true,
                truncated: false,
                exit: None,
            };
            Self::publish_tool_outcome(history, observer, &call.id, outcome);
            return;
        }

        observer.on_status(AgentStatus::ExecutingTool(call.name.clone()));
        let outcome = tool.execute(call.arguments.clone(), ctx).await;
        Self::publish_tool_outcome(history, observer, &call.id, outcome);
    }

    /// 连续 ParallelSafe 段：先按模型顺序发全部 started + ExecutingTools(total)，
    /// 再以 indexed stream + `buffer_unordered(MAX_PARALLEL_TOOL_CALLS)` 执行；
    /// ready buffer 只发布连续 ready 前缀（模型公开 occurrence 顺序）。
    async fn execute_parallel_safe_batch(
        &self,
        batch: &[ToolCall],
        history: &mut Vec<Message>,
        ctx: &ToolContext,
        observer: &dyn AgentObserver,
    ) {
        use futures_util::stream::{self, StreamExt as _};

        for call in batch {
            // eligible 保证 ReadOnly
            observer.on_tool_call_started(&call.id, &call.name, &call.arguments, true);
        }
        observer.on_status(AgentStatus::ExecutingTools(batch.len()));

        // ReadOnly gate 恒 Allow；启动前串行过 gate 保持契约，不并发授权。
        for call in batch {
            let tool = self
                .registry
                .get(&call.name)
                .expect("parallel eligible implies tool exists");
            let _ = gate(call, tool, self.decider.as_ref()).await;
        }

        let call_ids: Vec<String> = batch.iter().map(|c| c.id.clone()).collect();
        let n = batch.len();
        let mut ready: Vec<Option<ToolOutcome>> = (0..n).map(|_| None).collect();
        let mut next_publish = 0usize;

        // 每个 future 返回 (original_index, ToolOutcome)；不要求 registry Arc。
        let execs = (0..n).map(|idx| {
            let call = &batch[idx];
            let tool = self
                .registry
                .get(&call.name)
                .expect("parallel eligible implies tool exists");
            let args = call.arguments.clone();
            async move { (idx, tool.execute(args, ctx).await) }
        });

        let mut completed = stream::iter(execs).buffer_unordered(MAX_PARALLEL_TOOL_CALLS);
        while let Some((idx, outcome)) = completed.next().await {
            ready[idx] = Some(outcome);
            while next_publish < n {
                match ready[next_publish].take() {
                    Some(outcome) => {
                        Self::publish_tool_outcome(
                            history,
                            observer,
                            &call_ids[next_publish],
                            outcome,
                        );
                        next_publish += 1;
                    }
                    // 非连续 ready 前缀：等更早 index 完成（非 dangling 退出）。
                    None => break,
                }
            }
        }
        debug_assert_eq!(
            next_publish, n,
            "buffer_unordered drain must publish every occurrence"
        );
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
    use crate::permission::{
        PermissionCheck, PermissionDecider, PermissionDecision, PermissionMode,
    };
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, Depth, FinishReason, ModelResponse, Provider, ThinkingBlock, ThinkingConfig,
        ToolCall, Usage,
    };
    use crate::tool::edit::WriteFileTool;
    use crate::tool::plan::{MockPlanApprover, Plan, PlanApprover, PlanDecision, SubmitPlanTool};
    use crate::tool::web::{WebError, WebFetchTool, WebFetcher, WebSearchTool};
    use crate::tool::{
        NetworkPermissionPreview, NetworkPermissionScope, PermissionLevel, Tool, ToolConcurrency,
        ToolContext, ToolOutcome, ToolRegistry,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

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
        async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    struct DenyAll;

    #[async_trait]
    impl PermissionDecider for DenyAll {
        async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
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

    struct NetworkTool {
        preview: NetworkPermissionPreview,
        executions: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct CountingFetcher {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl WebFetcher for CountingFetcher {
        async fn fetch(&self, _url: &str) -> Result<String, WebError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("<html></html>".to_string())
        }

        fn permission_scope(&self) -> NetworkPermissionScope {
            NetworkPermissionScope {
                max_redirects: 0,
                may_cross_origin: false,
                ssrf_each_hop: false,
            }
        }
    }

    impl NetworkTool {
        fn authorizable(executions: Arc<AtomicUsize>) -> Self {
            Self {
                preview: NetworkPermissionPreview {
                    authorizable: true,
                    full_args: json!({ "url": "https://example.com" }),
                    canonical_initial_target: Some("https://example.com/".to_string()),
                    scope: Some(NetworkPermissionScope {
                        max_redirects: 3,
                        may_cross_origin: true,
                        ssrf_each_hop: true,
                    }),
                    denial_reason: None,
                },
                executions,
            }
        }

        fn reject_only(reason: &str, executions: Arc<AtomicUsize>) -> Self {
            Self {
                preview: NetworkPermissionPreview {
                    authorizable: false,
                    full_args: json!({ "url": "bad" }),
                    canonical_initial_target: None,
                    scope: None,
                    denial_reason: Some(reason.to_string()),
                },
                executions,
            }
        }
    }

    #[async_trait]
    impl Tool for NetworkTool {
        fn name(&self) -> &str {
            "network"
        }

        fn description(&self) -> &str {
            "Requires network permission"
        }

        fn schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::Network
        }

        fn network_permission_preview(&self, _args: &Value) -> NetworkPermissionPreview {
            self.preview.clone()
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
            self.executions.fetch_add(1, Ordering::SeqCst);
            ToolOutcome {
                content: "network result".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        }
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

    fn registry_with_network_tool(tool: NetworkTool) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(tool)).unwrap();
        registry
    }

    fn registry_with_denied_web_tool(name: &str, calls: Arc<AtomicUsize>) -> ToolRegistry {
        let fetcher = CountingFetcher { calls };
        let mut registry = ToolRegistry::new();
        match name {
            "web_fetch" => registry.register(Box::new(WebFetchTool::new(Box::new(fetcher)))),
            "web_search" => registry.register(Box::new(WebSearchTool::new(Box::new(fetcher)))),
            other => panic!("unexpected web tool: {other}"),
        }
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_observed_parallel_batch_observer_sequence_and_provider_wait() {
        // Echo 是 ReadOnly 但默认 Exclusive；用 LatchTool ParallelSafe 验证整段观测顺序。
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "a"), install_latch(&latches, "b")];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe", latches, active, max_active,
            )))
            .unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "safe", "a"),
                tool_call("call-2", "safe", "b"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider.clone(),
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);
        let mut got0 = false;
        let mut got1 = false;
        while !got0 || !got1 {
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("early: {result:?}");
                }
                r = &mut e0, if !got0 => { r.unwrap(); got0 = true; }
                r = &mut e1, if !got1 => { r.unwrap(); got1 = true; }
            }
        }
        release_all_shared(&release_slots);
        assert_eq!(run.await.unwrap(), "done");
        cancel_wd.store(true, Ordering::SeqCst);

        let events = observer.events();
        let started: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ObservedEvent::ToolCallStarted { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(started, vec!["call-1", "call-2"]);
        let tools_status_pos = events
            .iter()
            .position(|e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTools(2))))
            .expect("ExecutingTools(2)");
        let first_started = events
            .iter()
            .position(|e| matches!(e, ObservedEvent::ToolCallStarted { id, .. } if id == "call-1"))
            .unwrap();
        let second_started = events
            .iter()
            .position(|e| matches!(e, ObservedEvent::ToolCallStarted { id, .. } if id == "call-2"))
            .unwrap();
        assert!(first_started < second_started && second_started < tools_status_pos);
        let finished: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ObservedEvent::ToolCallFinished { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(finished, vec!["call-1", "call-2"]);
        assert!(
            tools_status_pos
                < events
                    .iter()
                    .position(|e| matches!(e, ObservedEvent::ToolCallFinished { .. }))
                    .unwrap()
        );

        // 第二次 provider.complete 仅在全部 ToolResult 入 history 后
        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        let tool_results: Vec<_> = recorded[1]
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_results, vec!["call-1", "call-2"]);
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
    async fn run_observed_characterizes_network_as_non_readonly_and_reports_denial() {
        let executions = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "network-1".to_string(),
                name: "network".to_string(),
                arguments: json!({ "url": "bad" }),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            provider,
            registry_with_network_tool(NetworkTool::reject_only("invalid target", executions)),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let mut history = vec![Message::User("research".to_string())];

        assert_eq!(
            agent
                .run_observed(&mut history, &ctx(), &sink, &observer)
                .await
                .unwrap(),
            "recovered"
        );

        let events = observer.events();
        assert!(matches!(
            events.as_slice(),
            [
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::ToolCallStarted {
                    readonly: false,
                    ..
                },
                ObservedEvent::Status(AgentStatus::WaitingForPermission),
                ObservedEvent::ToolCallFinished {
                    outcome: ToolOutcome { is_error: true, .. },
                    ..
                },
                ObservedEvent::Status(AgentStatus::CallingModel),
                ObservedEvent::Status(AgentStatus::Idle),
            ]
        ));
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
    async fn denied_web_tools_never_call_their_fetcher_and_preserve_user_denial() {
        for (name, arguments) in [
            ("web_fetch", json!({ "url": "https://example.com" })),
            ("web_search", json!({ "query": "rust ownership" })),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let provider = Arc::new(MockProvider::new(vec![
                tool_response(vec![ToolCall {
                    id: format!("{name}-1"),
                    name: name.to_string(),
                    arguments,
                }]),
                response("recovered"),
            ]));
            let agent = Agent::new(
                provider,
                registry_with_denied_web_tool(name, calls.clone()),
                Box::new(DenyAll),
                "mock-model".to_string(),
                4,
            );
            let sink = NoopSink;
            let mut history = vec![Message::User("research".to_string())];

            assert_eq!(
                agent.run(&mut history, &ctx(), &sink).await.unwrap(),
                "recovered"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0, "{name}");
            assert!(matches!(
                &history[2],
                Message::ToolResult { content, is_error: true, .. }
                    if content == "user denied tool execution"
            ));
        }
    }

    #[tokio::test]
    async fn yolo_malformed_web_fetch_is_rejected_before_fetching() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "web-fetch-invalid".to_string(),
                name: "web_fetch".to_string(),
                arguments: json!({ "url": "http://[invalid" }),
            }]),
            response("recovered"),
        ]));
        let mut agent = Agent::new(
            provider,
            registry_with_denied_web_tool("web_fetch", calls.clone()),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(Arc::new(Mutex::new(PermissionMode::Yolo)));
        let sink = NoopSink;
        let mut history = vec![Message::User("research".to_string())];

        assert_eq!(
            agent.run(&mut history, &ctx(), &sink).await.unwrap(),
            "recovered"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &history[2],
            Message::ToolResult { content, is_error: true, .. }
                if content.starts_with("invalid url:")
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
    async fn agent_plan_mode_allows_authorizable_network_tools_after_permission() {
        let executions = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "network-1".to_string(),
                name: "network".to_string(),
                arguments: json!({ "url": "https://example.com" }),
            }]),
            response("done"),
        ]));
        let mut agent = Agent::new(
            provider,
            registry_with_network_tool(NetworkTool::authorizable(executions.clone())),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(Arc::new(Mutex::new(PermissionMode::Plan)));
        let sink = NoopSink;
        let mut history = vec![Message::User("research".to_string())];

        assert_eq!(
            agent.run(&mut history, &ctx(), &sink).await.unwrap(),
            "done"
        );
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &history[2],
            Message::ToolResult { content, is_error: false, .. } if content == "network result"
        ));
    }

    #[tokio::test]
    async fn agent_preserves_network_system_denial_reason_in_history() {
        let executions = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "network-1".to_string(),
                name: "network".to_string(),
                arguments: json!({ "url": "bad" }),
            }]),
            response("recovered"),
        ]));
        let agent = Agent::new(
            provider,
            registry_with_network_tool(NetworkTool::reject_only(
                "invalid network target",
                executions.clone(),
            )),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("research".to_string())];

        assert_eq!(
            agent.run(&mut history, &ctx(), &sink).await.unwrap(),
            "recovered"
        );
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &history[2],
            Message::ToolResult { content, is_error: true, .. }
                if content == "invalid network target"
        ));
    }

    #[tokio::test]
    async fn plan_instruction_describes_network_permission_and_validation_without_readonly_web_claim(
    ) {
        let mode = Arc::new(Mutex::new(PermissionMode::Plan));
        let provider = Arc::new(MockProvider::new(vec![response("planning")]));
        let mut agent = Agent::new(
            provider.clone(),
            registry_with_echo(),
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(mode);
        let sink = NoopSink;
        let mut history = vec![Message::User("plan this".to_string())];

        agent.run(&mut history, &ctx(), &sink).await.unwrap();

        let Message::System(instruction) = &provider.recorded_requests()[0].messages[0] else {
            panic!("Plan mode must inject a system instruction");
        };
        assert!(instruction.contains("Network"));
        assert!(instruction.contains("Edit"));
        assert!(instruction.contains("Execute"));
        assert!(instruction.contains("validation"));
        assert!(!instruction.contains("只读:read_file/grep/glob/web_*"));
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
    // --- Parallel batch scheduling (§3.2–3.3 RED) ---

    struct LatchWatch {
        entered_rx: Option<oneshot::Receiver<()>>,
        release_tx: Option<oneshot::Sender<()>>,
        completed_rx: Option<oneshot::Receiver<()>>,
    }

    impl LatchWatch {
        fn take_entered(&mut self) -> oneshot::Receiver<()> {
            self.entered_rx.take().expect("entered_rx")
        }

        fn take_completed(&mut self) -> oneshot::Receiver<()> {
            self.completed_rx.take().expect("completed_rx")
        }
    }

    struct LatchSlot {
        entered_tx: Option<oneshot::Sender<()>>,
        release_rx: Option<oneshot::Receiver<()>>,
        completed_tx: Option<oneshot::Sender<()>>,
    }

    fn install_latch(map: &Arc<Mutex<HashMap<String, LatchSlot>>>, key: &str) -> LatchWatch {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (completed_tx, completed_rx) = oneshot::channel();
        map.lock().unwrap().insert(
            key.to_string(),
            LatchSlot {
                entered_tx: Some(entered_tx),
                release_rx: Some(release_rx),
                completed_tx: Some(completed_tx),
            },
        );
        LatchWatch {
            entered_rx: Some(entered_rx),
            release_tx: Some(release_tx),
            completed_rx: Some(completed_rx),
        }
    }

    struct LatchTool {
        name: &'static str,
        latches: Arc<Mutex<HashMap<String, LatchSlot>>>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        concurrency: ToolConcurrency,
        permission: PermissionLevel,
        plan_only: bool,
        error_keys: Arc<Mutex<HashSet<String>>>,
    }

    impl LatchTool {
        fn new(
            name: &'static str,
            latches: Arc<Mutex<HashMap<String, LatchSlot>>>,
            active: Arc<AtomicUsize>,
            max_active: Arc<AtomicUsize>,
            concurrency: ToolConcurrency,
            permission: PermissionLevel,
            plan_only: bool,
        ) -> Self {
            Self {
                name,
                latches,
                active,
                max_active,
                concurrency,
                permission,
                plan_only,
                error_keys: Arc::new(Mutex::new(HashSet::new())),
            }
        }

        fn parallel_safe(
            name: &'static str,
            latches: Arc<Mutex<HashMap<String, LatchSlot>>>,
            active: Arc<AtomicUsize>,
            max_active: Arc<AtomicUsize>,
        ) -> Self {
            Self::new(
                name,
                latches,
                active,
                max_active,
                ToolConcurrency::ParallelSafe,
                PermissionLevel::ReadOnly,
                false,
            )
        }
    }

    #[async_trait]
    impl Tool for LatchTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "Controlled latch tool for concurrency tests"
        }

        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"]
            })
        }

        fn permission_level(&self) -> PermissionLevel {
            self.permission.clone()
        }

        fn concurrency(&self) -> ToolConcurrency {
            self.concurrency
        }

        fn plan_only(&self) -> bool {
            self.plan_only
        }

        async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
            let key = args["key"].as_str().unwrap_or("missing").to_string();
            let (entered_tx, release_rx, completed_tx) = {
                let mut map = self.latches.lock().unwrap();
                let slot = map
                    .remove(&key)
                    .unwrap_or_else(|| panic!("latch not installed for key={key}"));
                (
                    slot.entered_tx.expect("entered_tx"),
                    slot.release_rx.expect("release_rx"),
                    slot.completed_tx.expect("completed_tx"),
                )
            };

            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(now, Ordering::SeqCst);
            let _ = entered_tx.send(());
            let _ = release_rx.await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            let _ = completed_tx.send(());

            let is_error = self.error_keys.lock().unwrap().contains(&key);
            ToolOutcome {
                content: if is_error {
                    format!("error:{key}")
                } else {
                    format!("ok:{key}")
                },
                is_error,
                truncated: false,
                exit: None,
            }
        }
    }

    fn tool_call(id: &str, name: &str, key: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: json!({ "key": key }),
        }
    }

    fn share_releases(watches: &mut [LatchWatch]) -> Arc<Mutex<Vec<Option<oneshot::Sender<()>>>>> {
        Arc::new(Mutex::new(
            watches.iter_mut().map(|w| w.release_tx.take()).collect(),
        ))
    }

    fn release_shared(slots: &Arc<Mutex<Vec<Option<oneshot::Sender<()>>>>>, index: usize) {
        if let Some(tx) = slots.lock().unwrap().get_mut(index).and_then(|s| s.take()) {
            let _ = tx.send(());
        }
    }

    fn release_all_shared(slots: &Arc<Mutex<Vec<Option<oneshot::Sender<()>>>>>) {
        for slot in slots.lock().unwrap().iter_mut() {
            if let Some(tx) = slot.take() {
                let _ = tx.send(());
            }
        }
    }

    fn arm_os_watchdog(
        release_slots: Arc<Mutex<Vec<Option<oneshot::Sender<()>>>>>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    ) -> (
        std::thread::JoinHandle<()>,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_thread = fired.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            fired_for_thread.store(true, Ordering::SeqCst);
            release_all_shared(&release_slots);
        });
        (handle, fired)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_safe_batch_two_tools_overlap_before_release() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "a"), install_latch(&latches, "b")];
        let e0 = watches[0].take_entered();
        let e1 = watches[1].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches,
                active.clone(),
                max_active,
            )))
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "safe", "a"),
                tool_call("call-2", "safe", "b"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);

        // 先等 call-1 entered；在不 release 的窗口内 call-2 也应 entered（真重叠）。
        let mut e1 = e1;
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("agent finished before first entered: {result:?}");
            }
            r = e0 => { r.unwrap(); }
        }
        let mut saw_second = false;
        for _ in 0..200 {
            if e1.try_recv().is_ok() {
                saw_second = true;
                break;
            }
            if active.load(Ordering::SeqCst) >= 2 {
                saw_second = true;
                break;
            }
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("agent finished before overlap window: {result:?}");
                }
                _ = tokio::task::yield_now() => {}
            }
        }
        assert!(
            saw_second && active.load(Ordering::SeqCst) >= 2,
            "both ParallelSafe tools must be active before either release; active={}",
            active.load(Ordering::SeqCst)
        );

        release_all_shared(&release_slots);
        let text = run.await.unwrap();
        cancel_wd.store(true, Ordering::SeqCst);
        assert_eq!(text, "done");

        let events = observer.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTools(2)))),
            "batch of 2 must emit ExecutingTools(2), events={events:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_safe_batch_five_calls_max_active_four() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let keys = ["k1", "k2", "k3", "k4", "k5"];
        let mut watches: Vec<_> = keys.iter().map(|k| install_latch(&latches, k)).collect();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches,
                active.clone(),
                max_active.clone(),
            )))
            .unwrap();

        let calls: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| tool_call(&format!("call-{}", i + 1), "safe", k))
            .collect();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(calls),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);

        // 审查问题 #6.7：per-call entered ack，不得用固定次数 yield 猜时序。
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let mut e2 = watches[2].take_entered();
        let mut e3 = watches[3].take_entered();
        let mut e4 = watches[4].take_entered();
        let mut got = [false; 5];
        while got.iter().take(4).filter(|g| **g).count() < 4 {
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("agent finished before 4 entered: {result:?}");
                }
                r = &mut e0, if !got[0] => { r.unwrap(); got[0] = true; }
                r = &mut e1, if !got[1] => { r.unwrap(); got[1] = true; }
                r = &mut e2, if !got[2] => { r.unwrap(); got[2] = true; }
                r = &mut e3, if !got[3] => { r.unwrap(); got[3] = true; }
                r = &mut e4, if !got[4] => {
                    r.unwrap();
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("fifth call entered while window full");
                }
            }
        }
        assert_eq!(
            max_active.load(Ordering::SeqCst),
            4,
            "max-active must be 4 after four entered acks"
        );
        assert!(!got[4], "fifth must not have entered");
        // 短暂轮询仍不得收到 k5 entered
        for _ in 0..20 {
            if e4.try_recv().is_ok() {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("fifth entered while first four held");
            }
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("agent finished while holding first four: {result:?}");
                }
                _ = tokio::task::yield_now() => {}
            }
        }

        release_all_shared(&release_slots);
        let text = run.await.unwrap();
        cancel_wd.store(true, Ordering::SeqCst);
        assert_eq!(text, "done");

        let events = observer.events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTools(5)))),
            "batch of 5 must emit ExecutingTools(5), events={events:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn exclusive_forms_barrier_between_safe_batches() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![
            install_latch(&latches, "s1"),
            install_latch(&latches, "s2"),
            install_latch(&latches, "ex"),
            install_latch(&latches, "s4"),
        ];
        let e0 = watches[0].take_entered();
        let e1 = watches[1].take_entered();
        let mut e2 = watches[2].take_entered();
        let mut e3 = watches[3].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches.clone(),
                active.clone(),
                max_active.clone(),
            )))
            .unwrap();
        registry
            .register(Box::new(LatchTool::new(
                "exclusive",
                latches,
                active.clone(),
                max_active,
                ToolConcurrency::Exclusive,
                PermissionLevel::ReadOnly,
                false,
            )))
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "safe", "s1"),
                tool_call("call-2", "safe", "s2"),
                tool_call("call-3", "exclusive", "ex"),
                tool_call("call-4", "safe", "s4"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);

        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early finish: {result:?}");
            }
            _ = async {
                e0.await.unwrap();
                e1.await.unwrap();
            } => {}
        }
        assert_eq!(active.load(Ordering::SeqCst), 2);

        for _ in 0..30 {
            if e2.try_recv().is_ok() || e3.try_recv().is_ok() {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("barrier crossed before safe batch finished");
            }
            tokio::task::yield_now().await;
        }

        release_shared(&release_slots, 0);
        release_shared(&release_slots, 1);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early finish waiting exclusive: {result:?}");
            }
            r = &mut e2 => { r.unwrap(); }
        }
        for _ in 0..30 {
            if e3.try_recv().is_ok() {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("safe-4 crossed exclusive barrier");
            }
            tokio::task::yield_now().await;
        }
        release_shared(&release_slots, 2);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early finish waiting s4: {result:?}");
            }
            r = &mut e3 => { r.unwrap(); }
        }
        release_shared(&release_slots, 3);
        assert_eq!(run.await.unwrap(), "done");
        cancel_wd.store(true, Ordering::SeqCst);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn single_parallel_safe_keeps_executing_tool_status() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "only")];
        let mut entered = watches[0].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe", latches, active, max_active,
            )))
            .unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![tool_call("call-1", "safe", "only")]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early: {result:?}");
            }
            r = &mut entered => { r.unwrap(); }
        }
        release_all_shared(&release_slots);
        assert_eq!(run.await.unwrap(), "done");
        cancel_wd.store(true, Ordering::SeqCst);
        let events = observer.events();
        assert!(
            events.iter().any(
                |e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTool(n)) if n == "safe")
            ),
            "single safe call must use ExecutingTool(name), events={events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTools(_)))),
            "single call must not emit ExecutingTools"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mislabeled_network_parallel_safe_is_clamped_exclusive() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::new(
                "net_tool",
                latches.clone(),
                active.clone(),
                max_active.clone(),
                ToolConcurrency::ParallelSafe,
                PermissionLevel::Network,
                false,
            )))
            .unwrap();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches.clone(),
                active,
                max_active,
            )))
            .unwrap();
        let mut watches = vec![install_latch(&latches, "s")];
        let release_slots = share_releases(&mut watches);
        release_all_shared(&release_slots);

        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "net_tool", "net"),
                tool_call("call-2", "safe", "s"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let text = agent
            .run_observed(&mut history, &ctx(), &sink, &observer)
            .await
            .unwrap();
        assert_eq!(text, "done");
        let events = observer.events();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ObservedEvent::Status(AgentStatus::ExecutingTools(_)))),
            "Network tool must not join ParallelSafe batch, events={events:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn work_conserving_refill_starts_fifth_while_first_held() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let keys = ["k1", "k2", "k3", "k4", "k5"];
        let mut watches: Vec<_> = keys.iter().map(|k| install_latch(&latches, k)).collect();
        let mut e1 = watches[0].take_entered();
        let mut e2 = watches[1].take_entered();
        let mut e3 = watches[2].take_entered();
        let mut e4 = watches[3].take_entered();
        let mut e5 = watches[4].take_entered();
        let mut c1 = watches[0].take_completed();
        let mut c2 = watches[1].take_completed();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (_wd, watchdog_fired) = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches,
                active.clone(),
                max_active.clone(),
            )))
            .unwrap();
        let calls: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| tool_call(&format!("c{}", i + 1), "safe", k))
            .collect();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(calls),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let done_text = {
            let tool_ctx = ctx();
            let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
            tokio::pin!(run);

            // 等前四个均 entered（无序，用 select 轮询）。
            let mut got = [false; 4];
            while got.iter().any(|g| !*g) {
                tokio::select! {
                    result = &mut run => {
                        release_all_shared(&release_slots);
                        cancel_wd.store(true, Ordering::SeqCst);
                        panic!("early before window full: {result:?}");
                    }
                    r = &mut e1, if !got[0] => { r.unwrap(); got[0] = true; }
                    r = &mut e2, if !got[1] => { r.unwrap(); got[1] = true; }
                    r = &mut e3, if !got[2] => { r.unwrap(); got[2] = true; }
                    r = &mut e4, if !got[3] => { r.unwrap(); got[3] = true; }
                }
            }
            assert_eq!(max_active.load(Ordering::SeqCst), 4);

            // 释放 k2，保持 k1；work-conserving 应启动 k5。
            release_shared(&release_slots, 1);
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("early waiting c2: {result:?}");
                }
                r = &mut c2 => { r.unwrap(); }
            }
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("finished before k5 entered: {result:?}");
                }
                r = &mut e5 => { r.unwrap(); }
            }
            assert!(
                !watchdog_fired.load(Ordering::SeqCst),
                "call-5 entered only after failure watchdog released call-1"
            );
            assert!(
                matches!(c1.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
                "call-1 must still be pending when work-conserving refill starts call-5"
            );
            assert!(
                active.load(Ordering::SeqCst) <= 4,
                "active still capped while k1 held and k5 running"
            );

            release_all_shared(&release_slots);
            run.await.unwrap()
        };
        assert_eq!(done_text, "done");
        cancel_wd.store(true, Ordering::SeqCst);

        let results: Vec<_> = history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id, content, ..
                } => Some((call_id.as_str(), content.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("c1", "ok:k1"),
                ("c2", "ok:k2"),
                ("c3", "ok:k3"),
                ("c4", "ok:k4"),
                ("c5", "ok:k5"),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reverse_physical_completion_still_publishes_model_order() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "a"), install_latch(&latches, "b")];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let mut c1 = watches[1].take_completed();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe", latches, active, max_active,
            )))
            .unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "safe", "a"),
                tool_call("call-2", "safe", "b"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider.clone(),
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let done_text = {
            let tool_ctx = ctx();
            let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
            tokio::pin!(run);

            let mut got0 = false;
            let mut got1 = false;
            while !got0 || !got1 {
                tokio::select! {
                    result = &mut run => {
                        release_all_shared(&release_slots);
                        cancel_wd.store(true, Ordering::SeqCst);
                        panic!("early: {result:?}");
                    }
                    r = &mut e0, if !got0 => { r.unwrap(); got0 = true; }
                    r = &mut e1, if !got1 => { r.unwrap(); got1 = true; }
                }
            }
            // 物理逆序：先完成 call-2，再 call-1
            release_shared(&release_slots, 1);
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("early after release-2: {result:?}");
                }
                r = &mut c1 => { r.unwrap(); }
            }
            release_shared(&release_slots, 0);
            run.await.unwrap()
        };
        assert_eq!(done_text, "done");
        cancel_wd.store(true, Ordering::SeqCst);

        let tool_results: Vec<_> = history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id, content, ..
                } => Some((call_id.clone(), content.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            tool_results,
            vec![
                ("call-1".into(), "ok:a".into()),
                ("call-2".into(), "ok:b".into()),
            ]
        );

        let finished_ids: Vec<_> = observer
            .events()
            .into_iter()
            .filter_map(|e| match e {
                ObservedEvent::ToolCallFinished { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(
            finished_ids,
            vec!["call-1".to_string(), "call-2".to_string()]
        );

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        let second_tool_ids: Vec<_> = recorded[1]
            .messages
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(second_tool_ids, vec!["call-1", "call-2"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn single_error_does_not_cancel_sibling_calls() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![
            install_latch(&latches, "a"),
            install_latch(&latches, "b"),
            install_latch(&latches, "c"),
        ];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let mut e2 = watches[2].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        let tool = LatchTool::parallel_safe("safe", latches, active.clone(), max_active);
        tool.error_keys.lock().unwrap().insert("b".into());
        registry.register(Box::new(tool)).unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("c1", "safe", "a"),
                tool_call("c2", "safe", "b"),
                tool_call("c3", "safe", "c"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let mut c_err = watches[1].take_completed();
        let done_text = {
            let tool_ctx = ctx();
            let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
            tokio::pin!(run);
            let mut got = [false; 3];
            while got.iter().any(|g| !*g) {
                tokio::select! {
                    result = &mut run => {
                        release_all_shared(&release_slots);
                        cancel_wd.store(true, Ordering::SeqCst);
                        panic!("early: {result:?}");
                    }
                    r = &mut e0, if !got[0] => { r.unwrap(); got[0] = true; }
                    r = &mut e1, if !got[1] => { r.unwrap(); got[1] = true; }
                    r = &mut e2, if !got[2] => { r.unwrap(); got[2] = true; }
                }
            }
            // 审查问题 #6.6：先只 release error occurrence，确认兄弟仍 pending。
            release_shared(&release_slots, 1);
            tokio::select! {
                result = &mut run => {
                    release_all_shared(&release_slots);
                    cancel_wd.store(true, Ordering::SeqCst);
                    panic!("agent finished after only error released: {result:?}");
                }
                r = &mut c_err => { r.unwrap(); }
            }
            // error 完成后，a/c 仍应 active（未 release）
            assert!(
                active.load(Ordering::SeqCst) >= 2,
                "siblings must still be pending after error completes; active={}",
                active.load(Ordering::SeqCst)
            );
            release_shared(&release_slots, 0);
            release_shared(&release_slots, 2);
            run.await.unwrap()
        };
        assert_eq!(done_text, "done");
        cancel_wd.store(true, Ordering::SeqCst);

        let results: Vec<_> = history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => Some((call_id.as_str(), content.as_str(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("c1", "ok:a", false),
                ("c2", "error:b", true),
                ("c3", "ok:c", false),
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_tool_forms_barrier() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "a"), install_latch(&latches, "c")];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe", latches, active, max_active,
            )))
            .unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("c1", "safe", "a"),
                ToolCall {
                    id: "c2".into(),
                    name: "missing_tool".into(),
                    arguments: json!({}),
                },
                tool_call("c3", "safe", "c"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);

        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early: {result:?}");
            }
            r = &mut e0 => { r.unwrap(); }
        }
        for _ in 0..30 {
            if e1.try_recv().is_ok() {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("safe-c overlapped across unknown barrier");
            }
            tokio::task::yield_now().await;
        }
        release_shared(&release_slots, 0);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early waiting c: {result:?}");
            }
            r = &mut e1 => { r.unwrap(); }
        }
        release_shared(&release_slots, 1);
        assert_eq!(run.await.unwrap(), "done");
        cancel_wd.store(true, Ordering::SeqCst);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plan_only_parallel_safe_still_forms_barrier_in_plan_mode() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![
            install_latch(&latches, "a"),
            install_latch(&latches, "plan"),
            install_latch(&latches, "c"),
        ];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let mut e2 = watches[2].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe",
                latches.clone(),
                active.clone(),
                max_active.clone(),
            )))
            .unwrap();
        registry
            .register(Box::new(LatchTool::new(
                "plan_tool",
                latches,
                active,
                max_active,
                ToolConcurrency::ParallelSafe,
                PermissionLevel::ReadOnly,
                true,
            )))
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("c1", "safe", "a"),
                tool_call("c2", "plan_tool", "plan"),
                tool_call("c3", "safe", "c"),
            ]),
            response("done"),
        ]));
        let mut agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        agent.set_permission_mode(Arc::new(Mutex::new(PermissionMode::Plan)));
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let tool_ctx = ctx();
        let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
        tokio::pin!(run);

        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early: {result:?}");
            }
            r = &mut e0 => { r.unwrap(); }
        }
        for _ in 0..30 {
            if e2.try_recv().is_ok() {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("safe-c crossed plan_only barrier");
            }
            tokio::task::yield_now().await;
        }
        release_shared(&release_slots, 0);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early plan: {result:?}");
            }
            r = &mut e1 => { r.unwrap(); }
        }
        release_shared(&release_slots, 1);
        tokio::select! {
            result = &mut run => {
                release_all_shared(&release_slots);
                cancel_wd.store(true, Ordering::SeqCst);
                panic!("early c: {result:?}");
            }
            r = &mut e2 => { r.unwrap(); }
        }
        release_shared(&release_slots, 2);
        assert_eq!(run.await.unwrap(), "done");
        cancel_wd.store(true, Ordering::SeqCst);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn duplicate_call_ids_produce_two_results_by_occurrence() {
        let latches = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let mut watches = vec![install_latch(&latches, "a"), install_latch(&latches, "b")];
        let mut e0 = watches[0].take_entered();
        let mut e1 = watches[1].take_entered();
        let release_slots = share_releases(&mut watches);
        let cancel_wd = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let _wd = arm_os_watchdog(release_slots.clone(), cancel_wd.clone());

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(LatchTool::parallel_safe(
                "safe", latches, active, max_active,
            )))
            .unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            tool_response(vec![
                tool_call("call-1", "safe", "a"),
                tool_call("call-1", "safe", "b"),
            ]),
            response("done"),
        ]));
        let agent = Agent::new(
            provider,
            registry,
            Box::new(AllowAll),
            "mock-model".to_string(),
            4,
        );
        let mut history = vec![Message::User("go".into())];
        let sink = NoopSink;
        let observer = RecordingObserver::default();
        let done_text = {
            let tool_ctx = ctx();
            let run = agent.run_observed(&mut history, &tool_ctx, &sink, &observer);
            tokio::pin!(run);
            let mut got0 = false;
            let mut got1 = false;
            while !got0 || !got1 {
                tokio::select! {
                    result = &mut run => {
                        release_all_shared(&release_slots);
                        cancel_wd.store(true, Ordering::SeqCst);
                        panic!("early: {result:?}");
                    }
                    r = &mut e0, if !got0 => { r.unwrap(); got0 = true; }
                    r = &mut e1, if !got1 => { r.unwrap(); got1 = true; }
                }
            }
            release_all_shared(&release_slots);
            run.await.unwrap()
        };
        assert_eq!(done_text, "done");
        cancel_wd.store(true, Ordering::SeqCst);

        let results: Vec<_> = history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id, content, ..
                } => Some((call_id.as_str(), content.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![("call-1", "ok:a"), ("call-1", "ok:b")],
            "duplicate ids must yield one ToolResult per occurrence"
        );
        let finished = observer
            .events()
            .into_iter()
            .filter(|e| matches!(e, ObservedEvent::ToolCallFinished { .. }))
            .count();
        assert_eq!(finished, 2);
    }
}
