use super::super::{DelegateTaskTool, DELEGATE_TASK_NAME};
use crate::agent::message::Message;
use crate::agent::{
    Agent, AgentExecutionScope, AgentObserver, AgentRuntime, AgentStatus, RunIdentity,
    ScopedAgentError,
};
use crate::error::ProviderError;
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{
    DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall, Usage,
};
use crate::tool::{
    BlockingToolLimiter, PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolOutcome,
    ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const WATCHDOG: Duration = Duration::from_secs(5);
const CANCELLED_RESULT: &str = "tool call interrupted before completion";

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

struct DenyAll;

#[async_trait]
impl PermissionDecider for DenyAll {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        PermissionDecision::Deny
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ObservedEvent {
    Status(RunIdentity, AgentStatus),
    Started(RunIdentity, String),
    Finished(RunIdentity, String),
    Usage(RunIdentity, Usage),
}

#[derive(Default)]
struct RecordingObserver {
    events: Mutex<Vec<ObservedEvent>>,
}

impl RecordingObserver {
    fn events(&self) -> Vec<ObservedEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentObserver for RecordingObserver {
    fn on_scoped_status(&self, identity: &RunIdentity, status: AgentStatus) {
        self.events
            .lock()
            .unwrap()
            .push(ObservedEvent::Status(*identity, status));
    }

    fn on_scoped_tool_call_started(
        &self,
        identity: &RunIdentity,
        id: &str,
        _name: &str,
        _args: &Value,
        _readonly: bool,
    ) {
        self.events
            .lock()
            .unwrap()
            .push(ObservedEvent::Started(*identity, id.to_string()));
    }

    fn on_scoped_tool_call_finished(
        &self,
        identity: &RunIdentity,
        id: &str,
        _outcome: &ToolOutcome,
    ) {
        self.events
            .lock()
            .unwrap()
            .push(ObservedEvent::Finished(*identity, id.to_string()));
    }

    fn on_scoped_usage(&self, identity: &RunIdentity, usage: &Usage) {
        self.events
            .lock()
            .unwrap()
            .push(ObservedEvent::Usage(*identity, usage.clone()));
    }
}

struct SettledSignal(Option<oneshot::Sender<()>>);

impl Drop for SettledSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

struct PendingProviderSlot {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<ModelResponse>,
    settled: oneshot::Sender<()>,
}

struct PendingProvider {
    slots: Mutex<VecDeque<PendingProviderSlot>>,
}

struct PendingProviderWatch {
    entered: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<ModelResponse>>,
    settled: Option<oneshot::Receiver<()>>,
}

impl PendingProvider {
    fn controlled(count: usize) -> (Arc<Self>, Vec<PendingProviderWatch>) {
        let mut slots = VecDeque::new();
        let mut watches = Vec::new();
        for _ in 0..count {
            let (entered_tx, entered_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            let (settled_tx, settled_rx) = oneshot::channel();
            slots.push_back(PendingProviderSlot {
                entered: entered_tx,
                release: release_rx,
                settled: settled_tx,
            });
            watches.push(PendingProviderWatch {
                entered: Some(entered_rx),
                release: Some(release_tx),
                settled: Some(settled_rx),
            });
        }
        (
            Arc::new(Self {
                slots: Mutex::new(slots),
            }),
            watches,
        )
    }
}

#[async_trait]
impl Provider for PendingProvider {
    fn name(&self) -> &str {
        "pending-child"
    }

    async fn complete(
        &self,
        _req: ModelRequest,
        _sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        let slot = self
            .slots
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected child Provider call");
        let _settled = SettledSignal(Some(slot.settled));
        let _ = slot.entered.send(());
        slot.release
            .await
            .map_err(|_| ProviderError::Transport("child Provider release dropped".to_string()))
    }
}

struct ProviderReleaseGuard {
    senders: Vec<Option<oneshot::Sender<ModelResponse>>>,
}

impl ProviderReleaseGuard {
    fn from_watches(watches: &mut [PendingProviderWatch]) -> Self {
        Self {
            senders: watches
                .iter_mut()
                .map(|watch| watch.release.take())
                .collect(),
        }
    }
}

impl Drop for ProviderReleaseGuard {
    fn drop(&mut self) {
        for sender in &mut self.senders {
            if let Some(sender) = sender.take() {
                let _ = sender.send(final_response("cleanup"));
            }
        }
    }
}

struct HeldToolSlot {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
    settled: oneshot::Sender<()>,
}

#[derive(Default)]
struct HeldToolControl {
    slots: Mutex<VecDeque<HeldToolSlot>>,
}

struct HeldToolWatch {
    entered: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<()>>,
    settled: Option<oneshot::Receiver<()>>,
}

impl HeldToolControl {
    fn install(&self) -> HeldToolWatch {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (settled_tx, settled_rx) = oneshot::channel();
        self.slots.lock().unwrap().push_back(HeldToolSlot {
            entered: entered_tx,
            release: release_rx,
            settled: settled_tx,
        });
        HeldToolWatch {
            entered: Some(entered_rx),
            release: Some(release_tx),
            settled: Some(settled_rx),
        }
    }

    async fn execute(&self) -> ToolOutcome {
        let slot = self
            .slots
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected controlled read_file call");
        let _settled = SettledSignal(Some(slot.settled));
        let _ = slot.entered.send(());
        let _ = slot.release.await;
        ToolOutcome {
            content: "controlled read completed".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct HeldToolReleaseGuard {
    senders: Vec<Option<oneshot::Sender<()>>>,
}

impl HeldToolReleaseGuard {
    fn from_watches(watches: &mut [HeldToolWatch]) -> Self {
        Self {
            senders: watches
                .iter_mut()
                .map(|watch| watch.release.take())
                .collect(),
        }
    }
}

impl Drop for HeldToolReleaseGuard {
    fn drop(&mut self) {
        for sender in &mut self.senders {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
        }
    }
}

struct HeldReadFile {
    control: Arc<HeldToolControl>,
}

#[async_trait]
impl Tool for HeldReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Deterministic held read_file cancellation probe."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        self.control.execute().await
    }
}

type RunOutput = (Result<String, ScopedAgentError>, Vec<Message>);

struct RunDriver {
    handle: Option<JoinHandle<RunOutput>>,
}

impl RunDriver {
    fn spawn(
        agent: Arc<Agent>,
        scope: AgentExecutionScope,
        context: ToolContext,
        observer: Arc<RecordingObserver>,
    ) -> Self {
        let handle = tokio::spawn(async move {
            let mut history = vec![Message::User("delegate cancellation probe".to_string())];
            let result = agent
                .run_observed_scoped(&scope, &mut history, &context, &NoopSink, observer.as_ref())
                .await;
            (result, history)
        });
        Self {
            handle: Some(handle),
        }
    }

    async fn finish(&mut self) -> RunOutput {
        let handle = self.handle.as_mut().expect("run driver handle");
        let result = tokio::time::timeout(WATCHDOG, handle)
            .await
            .expect("cancelled outer run did not settle")
            .expect("outer run task panicked");
        self.handle.take();
        result
    }
}

impl Drop for RunDriver {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

fn tool_response(calls: Vec<ToolCall>) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: calls,
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

fn delegate_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: DELEGATE_TASK_NAME.to_string(),
        arguments: json!({ "task": format!("inspect {id}") }),
    }
}

fn read_call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: json!({ "path": path }),
    }
}

fn context(cwd: impl Into<PathBuf>) -> ToolContext {
    ToolContext {
        cwd: cwd.into(),
        max_output_bytes: 4096,
    }
}

fn cancellation_results(history: &[Message]) -> Vec<(String, String, bool)> {
    history
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

fn assert_no_late_or_ordinary_events(
    before_cancel: &[ObservedEvent],
    observer: &RecordingObserver,
) {
    let after_cancel = observer.events();
    assert_eq!(
        after_cancel, before_cancel,
        "cancellation must not publish late status/tool/usage events"
    );
    assert!(
        !after_cancel
            .iter()
            .any(|event| matches!(event, ObservedEvent::Finished(..))),
        "ordinary tool-finished events must be discarded"
    );
    assert!(
        !after_cancel
            .iter()
            .any(|event| matches!(event, ObservedEvent::Usage(..))),
        "cancelled child must not publish late usage"
    );
    assert!(
        !after_cancel
            .iter()
            .any(|event| matches!(event, ObservedEvent::Status(_, AgentStatus::Idle))),
        "cancelled outer or child run must not publish Idle"
    );
}

async fn assert_fresh_scope_succeeds(agent: &Agent, cwd: &Path, expected: &str) {
    let scope = agent.product_root_scope();
    let observer = RecordingObserver::default();
    let mut history = vec![Message::User("fresh scope".to_string())];
    let result = agent
        .run_observed_scoped(&scope, &mut history, &context(cwd), &NoopSink, &observer)
        .await;
    assert_eq!(result, Ok(expected.to_string()));
    assert!(
        observer.events().iter().any(|event| matches!(
            event,
            ObservedEvent::Status(identity, AgentStatus::Idle)
                if *identity == scope.identity()
        )),
        "fresh scope must reach Idle"
    );
}

fn delegate_with_child_registry(
    child_provider: Arc<dyn Provider>,
    child_registry: ToolRegistry,
) -> DelegateTaskTool {
    DelegateTaskTool::with_dependencies(
        AgentRuntime::new(child_provider, "child-model".to_string()),
        child_registry,
        Arc::new(|path: &Path| std::fs::canonicalize(path)),
        BlockingToolLimiter::new(4),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_real_delegates_cancel_four_child_providers_and_one_waiting_slot_without_late_events()
{
    let workspace = tempfile::tempdir().unwrap();
    let (child_provider, mut watches) = PendingProvider::controlled(5);
    let _releases = ProviderReleaseGuard::from_watches(&mut watches);
    let ids = ["outer-1", "outer-2", "outer-3", "outer-4", "outer-5"];
    let outer_provider = Arc::new(MockProvider::new(vec![
        tool_response(ids.iter().map(|id| delegate_call(id)).collect()),
        final_response("fresh outer turn"),
    ]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(delegate_with_child_registry(
            child_provider,
            ToolRegistry::new(),
        )))
        .unwrap();
    let agent = Arc::new(Agent::new(
        outer_provider.clone(),
        registry,
        Box::new(DenyAll),
        "outer-model".to_string(),
        4,
    ));
    let scope = agent.product_root_scope();
    let observer = Arc::new(RecordingObserver::default());
    let mut driver = RunDriver::spawn(
        agent.clone(),
        scope.clone(),
        context(workspace.path()),
        observer.clone(),
    );

    for watch in watches.iter_mut().take(4) {
        watch
            .entered
            .take()
            .expect("entered receiver")
            .await
            .expect("first four child Providers must enter");
    }
    assert!(
        matches!(
            watches[4]
                .entered
                .as_mut()
                .expect("fifth entered receiver")
                .try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "fifth delegate must remain outside the four scheduler slots"
    );
    let before_cancel = observer.events();
    scope.cancel();
    let (result, history) = driver.finish().await;
    for watch in watches.iter_mut().take(4) {
        watch
            .settled
            .take()
            .expect("settled receiver")
            .await
            .expect("entered child Provider future must be dropped");
    }
    assert!(
        matches!(
            watches[4]
                .settled
                .as_mut()
                .expect("fifth settled receiver")
                .try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ),
        "fifth child Provider future must never be created"
    );

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    let results = cancellation_results(&history);
    assert_eq!(
        results,
        ids.iter()
            .map(|id| (id.to_string(), CANCELLED_RESULT.to_string(), true))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        results
            .iter()
            .map(|(id, _, _)| id)
            .collect::<HashSet<_>>()
            .len(),
        5,
        "all five outer occurrences need one unique synthetic result"
    );
    assert_no_late_or_ordinary_events(&before_cancel, observer.as_ref());
    assert_fresh_scope_succeeds(agent.as_ref(), workspace.path(), "fresh outer turn").await;
    assert_eq!(outer_provider.recorded_requests().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_delegate_cancels_parallel_child_reads_without_finished_events_and_recovers() {
    let workspace = tempfile::tempdir().unwrap();
    let control = Arc::new(HeldToolControl::default());
    let mut watches = vec![control.install(), control.install()];
    let _releases = HeldToolReleaseGuard::from_watches(&mut watches);
    let child_provider = Arc::new(MockProvider::new(vec![tool_response(vec![
        read_call("child-read-1", "one.txt"),
        read_call("child-read-2", "two.txt"),
    ])]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(HeldReadFile {
            control: control.clone(),
        }))
        .unwrap();
    let child_registry = registry.restricted_to(["read_file"]).unwrap();
    registry
        .register(Box::new(delegate_with_child_registry(
            child_provider.clone(),
            child_registry,
        )))
        .unwrap();
    let outer_provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![delegate_call("outer-delegate")]),
        final_response("fresh after child reads"),
    ]));
    let agent = Arc::new(Agent::new(
        outer_provider.clone(),
        registry,
        Box::new(DenyAll),
        "outer-model".to_string(),
        4,
    ));
    let scope = agent.product_root_scope();
    let observer = Arc::new(RecordingObserver::default());
    let mut driver = RunDriver::spawn(
        agent.clone(),
        scope.clone(),
        context(workspace.path()),
        observer.clone(),
    );

    for watch in &mut watches {
        watch
            .entered
            .take()
            .expect("read entered receiver")
            .await
            .expect("both child reads must enter");
    }
    let before_cancel = observer.events();
    scope.cancel();
    let (result, history) = driver.finish().await;
    for watch in &mut watches {
        watch
            .settled
            .take()
            .expect("read settled receiver")
            .await
            .expect("held child read future must be dropped");
    }

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(
        cancellation_results(&history),
        vec![(
            "outer-delegate".to_string(),
            CANCELLED_RESULT.to_string(),
            true,
        )]
    );
    assert_no_late_or_ordinary_events(&before_cancel, observer.as_ref());
    assert_eq!(
        child_provider.recorded_requests().len(),
        1,
        "child must reach exactly the held parallel read batch"
    );
    assert_fresh_scope_succeeds(agent.as_ref(), workspace.path(), "fresh after child reads").await;
    assert_eq!(outer_provider.recorded_requests().len(), 2);
}
