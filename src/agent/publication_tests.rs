use super::{
    Agent, AgentExecutionScope, AgentObserver, AgentStatus, ExecutionBudget, ExecutionCapabilities,
    RunIdentity, ScopedAgentError,
};
use crate::agent::message::Message;
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall, Usage};
use crate::tool::{
    PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolExecutionContext, ToolOutcome,
    ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

const SERIAL_TOOL: &str = "serial_cancel_ready";
const PARALLEL_TOOL: &str = "parallel_cancel_ready";
const CANCELLED_RESULT: &str = "tool call interrupted before completion";

struct AllowAll;

#[async_trait]
impl PermissionDecider for AllowAll {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ObservedEvent {
    Status {
        identity: RunIdentity,
        status: AgentStatus,
    },
    Started {
        identity: RunIdentity,
        call_id: String,
    },
    Finished {
        identity: RunIdentity,
        call_id: String,
        outcome: ToolOutcome,
    },
    Usage {
        identity: RunIdentity,
        usage: Usage,
    },
}

impl ObservedEvent {
    fn identity(&self) -> RunIdentity {
        match self {
            Self::Status { identity, .. }
            | Self::Started { identity, .. }
            | Self::Finished { identity, .. }
            | Self::Usage { identity, .. } => *identity,
        }
    }
}

#[derive(Default)]
struct RecordingObserver {
    events: Mutex<Vec<ObservedEvent>>,
}

impl RecordingObserver {
    fn for_identity(&self, identity: RunIdentity) -> Vec<ObservedEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.identity() == identity)
            .cloned()
            .collect()
    }
}

impl AgentObserver for RecordingObserver {
    fn on_scoped_status(&self, identity: &RunIdentity, status: AgentStatus) {
        self.events.lock().unwrap().push(ObservedEvent::Status {
            identity: *identity,
            status,
        });
    }

    fn on_scoped_tool_call_started(
        &self,
        identity: &RunIdentity,
        id: &str,
        _name: &str,
        _args: &Value,
        _readonly: bool,
    ) {
        self.events.lock().unwrap().push(ObservedEvent::Started {
            identity: *identity,
            call_id: id.to_string(),
        });
    }

    fn on_scoped_tool_call_finished(
        &self,
        identity: &RunIdentity,
        id: &str,
        outcome: &ToolOutcome,
    ) {
        self.events.lock().unwrap().push(ObservedEvent::Finished {
            identity: *identity,
            call_id: id.to_string(),
            outcome: outcome.clone(),
        });
    }

    fn on_scoped_usage(&self, identity: &RunIdentity, usage: &Usage) {
        self.events.lock().unwrap().push(ObservedEvent::Usage {
            identity: *identity,
            usage: usage.clone(),
        });
    }
}

#[derive(Default)]
struct RecordingSink {
    chunks: Mutex<Vec<String>>,
}

impl RecordingSink {
    fn has_nonempty_chunk(&self) -> bool {
        self.chunks
            .lock()
            .unwrap()
            .iter()
            .any(|chunk| !chunk.is_empty())
    }
}

impl DeltaSink for RecordingSink {
    fn on_text(&self, text: &str) {
        self.chunks.lock().unwrap().push(text.to_string());
    }
}

struct SerialCancelReadyTool;

#[async_trait]
impl Tool for SerialCancelReadyTool {
    fn name(&self) -> &str {
        SERIAL_TOOL
    }

    fn description(&self) -> &str {
        "Cancels the current scope and immediately returns an ordinary result."
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        panic!("publication race tool must use execute_scoped")
    }

    async fn execute_scoped(&self, _args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        ctx.scope.cancel();
        ordinary_outcome("ordinary serial result")
    }
}

struct ParallelControl {
    state: Mutex<ParallelControlState>,
}

struct ParallelControlState {
    first_entered: Option<oneshot::Sender<()>>,
    first_release: Option<oneshot::Receiver<()>>,
    second_ready: Option<oneshot::Sender<()>>,
}

struct ParallelCancelReadyTool {
    control: Arc<ParallelControl>,
}

#[async_trait]
impl Tool for ParallelCancelReadyTool {
    fn name(&self) -> &str {
        PARALLEL_TOOL
    }

    fn description(&self) -> &str {
        "Buffers the second result before the first cancels the scope."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "role": { "type": "string" } },
            "required": ["role"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        panic!("publication race tool must use execute_scoped")
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        match args["role"].as_str().expect("parallel race role") {
            "first" => {
                let (entered, release) = {
                    let mut state = self.control.state.lock().unwrap();
                    (
                        state.first_entered.take().expect("first entered sender"),
                        state.first_release.take().expect("first release receiver"),
                    )
                };
                let _ = entered.send(());
                release.await.expect("first release");
                ctx.scope.cancel();
                ordinary_outcome("ordinary first result")
            }
            "second" => {
                let ready = self
                    .control
                    .state
                    .lock()
                    .unwrap()
                    .second_ready
                    .take()
                    .expect("second ready sender");
                let _ = ready.send(());
                ordinary_outcome("ordinary second result")
            }
            role => panic!("unexpected parallel race role: {role}"),
        }
    }
}

struct ReleaseGuard(Option<oneshot::Sender<()>>);

impl ReleaseGuard {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        self.release();
    }
}

fn ordinary_outcome(content: &str) -> ToolOutcome {
    ToolOutcome {
        content: content.to_string(),
        is_error: false,
        truncated: false,
        exit: None,
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

fn final_response(text: &str) -> ModelResponse {
    ModelResponse {
        text: text.to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: None,
        thinking: Vec::new(),
    }
}

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    }
}

fn scope(tool_name: &str) -> AgentExecutionScope {
    AgentExecutionScope::root(
        ExecutionBudget::new(4, None, 0),
        ExecutionCapabilities::try_new([tool_name], [PermissionLevel::ReadOnly]).unwrap(),
    )
}

fn context() -> ToolContext {
    ToolContext {
        cwd: PathBuf::from("."),
        max_output_bytes: 4096,
    }
}

fn tool_results(messages: &[Message]) -> Vec<(String, String, bool)> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => Some((call_id.clone(), content.clone(), *is_error)),
            _ => None,
        })
        .collect()
}

fn assert_cancelled_run_events(
    observer: &RecordingObserver,
    identity: RunIdentity,
    expected_statuses: &[AgentStatus],
    expected_started_ids: &[&str],
) {
    let events = observer.for_identity(identity);
    let statuses = events
        .iter()
        .filter_map(|event| match event {
            ObservedEvent::Status { status, .. } => Some(status.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let started_ids = events
        .iter()
        .filter_map(|event| match event {
            ObservedEvent::Started { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(statuses, expected_statuses);
    assert_eq!(started_ids, expected_started_ids);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ObservedEvent::Finished { .. })),
        "ordinary tool-finished must not be published after cancellation: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ObservedEvent::Usage { .. })),
        "cancelled run must not publish late usage: {events:?}"
    );
    assert!(
        !statuses.contains(&AgentStatus::Idle),
        "cancelled run must not publish Idle"
    );
}

async fn run_fresh_turn(
    agent: &Agent,
    scope: &AgentExecutionScope,
    observer: &RecordingObserver,
    expected: &str,
) {
    let mut history = vec![Message::User("fresh turn".to_string())];
    let sink = RecordingSink::default();
    let result = agent
        .run_observed_scoped(scope, &mut history, &context(), &sink, observer)
        .await;

    assert_eq!(result, Ok(expected.to_string()));
    assert!(
        observer
            .for_identity(scope.identity())
            .iter()
            .any(|event| matches!(
                event,
                ObservedEvent::Status {
                    status: AgentStatus::Idle,
                    ..
                }
            )),
        "fresh scope must complete normally"
    );
}

#[tokio::test]
async fn serial_ready_outcome_is_discarded_when_tool_cancels_scope_before_publication() {
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("serial-race", SERIAL_TOOL, json!({}))]),
        final_response("fresh serial turn"),
    ]));
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(SerialCancelReadyTool)).unwrap();
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let first_scope = scope(SERIAL_TOOL);
    let observer = RecordingObserver::default();
    let sink = RecordingSink::default();
    let mut history = vec![Message::User("race".to_string())];

    let result = agent
        .run_observed_scoped(&first_scope, &mut history, &context(), &sink, &observer)
        .await;
    let fresh_scope = scope(SERIAL_TOOL);
    run_fresh_turn(&agent, &fresh_scope, &observer, "fresh serial turn").await;

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(
        tool_results(&history),
        vec![(
            "serial-race".to_string(),
            CANCELLED_RESULT.to_string(),
            true,
        )],
        "ready ordinary result must be replaced by the single synthetic termination result"
    );
    assert_cancelled_run_events(
        &observer,
        first_scope.identity(),
        &[
            AgentStatus::CallingModel,
            AgentStatus::ExecutingTool(SERIAL_TOOL.to_string()),
        ],
        &["serial-race"],
    );
    assert!(
        !sink.has_nonempty_chunk(),
        "cancelled run must not publish non-empty model text"
    );
    assert_eq!(provider.recorded_requests().len(), 2);
}

#[tokio::test]
async fn parallel_ready_buffer_is_discarded_when_prefix_cancels_scope_before_publication() {
    let (first_entered_tx, mut first_entered_rx) = oneshot::channel();
    let (first_release_tx, first_release_rx) = oneshot::channel();
    let (second_ready_tx, mut second_ready_rx) = oneshot::channel();
    let control = Arc::new(ParallelControl {
        state: Mutex::new(ParallelControlState {
            first_entered: Some(first_entered_tx),
            first_release: Some(first_release_rx),
            second_ready: Some(second_ready_tx),
        }),
    });
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![
            call("duplicate-race", PARALLEL_TOOL, json!({ "role": "first" })),
            call("duplicate-race", PARALLEL_TOOL, json!({ "role": "second" })),
        ]),
        final_response("fresh parallel turn"),
    ]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(ParallelCancelReadyTool { control }))
        .unwrap();
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let first_scope = scope(PARALLEL_TOOL);
    let observer = RecordingObserver::default();
    let sink = RecordingSink::default();
    let mut history = vec![Message::User("parallel race".to_string())];
    let mut release = ReleaseGuard(Some(first_release_tx));

    let result = {
        let tool_context = context();
        let mut run = Box::pin(agent.run_observed_scoped(
            &first_scope,
            &mut history,
            &tool_context,
            &sink,
            &observer,
        ));
        tokio::select! {
            result = &mut run => panic!("run ended before first occurrence entered: {result:?}"),
            entered = &mut first_entered_rx => entered.expect("first occurrence entered"),
        }
        tokio::select! {
            result = &mut run => panic!("run ended before second occurrence became ready: {result:?}"),
            ready = &mut second_ready_rx => ready.expect("second occurrence ready"),
        }
        release.release();
        (&mut run).await
    };
    let fresh_scope = scope(PARALLEL_TOOL);
    run_fresh_turn(&agent, &fresh_scope, &observer, "fresh parallel turn").await;

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(
        tool_results(&history),
        vec![
            (
                "duplicate-race".to_string(),
                CANCELLED_RESULT.to_string(),
                true,
            ),
            (
                "duplicate-race".to_string(),
                CANCELLED_RESULT.to_string(),
                true,
            ),
        ],
        "ready prefix and private buffered suffix must both be discarded by the post-ready checkpoint"
    );
    assert_cancelled_run_events(
        &observer,
        first_scope.identity(),
        &[AgentStatus::CallingModel, AgentStatus::ExecutingTools(2)],
        &["duplicate-race", "duplicate-race"],
    );
    assert!(
        !sink.has_nonempty_chunk(),
        "cancelled run must not publish non-empty model text"
    );
    assert_eq!(provider.recorded_requests().len(), 2);
}
