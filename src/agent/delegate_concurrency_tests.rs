use super::{Agent, AgentObserver, ScopedAgentError};
use crate::agent::message::Message;
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use crate::tool::{PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolOutcome, ToolRegistry};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const DELEGATE_TASK: &str = "delegate_task";

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

fn call(id: &str, name: &str, key: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: json!({ "key": key }),
    }
}

struct ControlSlot {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
    completed: oneshot::Sender<()>,
}

struct ControlWatch {
    entered: Option<oneshot::Receiver<()>>,
    release: Option<oneshot::Sender<()>>,
    completed: Option<oneshot::Receiver<()>>,
}

impl ControlWatch {
    fn take_entered(&mut self) -> oneshot::Receiver<()> {
        self.entered.take().expect("entered receiver")
    }

    fn take_completed(&mut self) -> oneshot::Receiver<()> {
        self.completed.take().expect("completed receiver")
    }
}

#[derive(Default)]
struct ToolControl {
    slots: Mutex<HashMap<String, ControlSlot>>,
    active: AtomicUsize,
    max_active: AtomicUsize,
    completion_order: Mutex<Vec<String>>,
}

impl ToolControl {
    fn install(&self, key: &str) -> ControlWatch {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (completed_tx, completed_rx) = oneshot::channel();
        self.slots.lock().unwrap().insert(
            key.to_string(),
            ControlSlot {
                entered: entered_tx,
                release: release_rx,
                completed: completed_tx,
            },
        );
        ControlWatch {
            entered: Some(entered_rx),
            release: Some(release_tx),
            completed: Some(completed_rx),
        }
    }

    async fn execute(&self, key: String) -> ToolOutcome {
        let slot = self
            .slots
            .lock()
            .unwrap()
            .remove(&key)
            .unwrap_or_else(|| panic!("missing control slot for {key}"));
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        let active_guard = ActiveGuard {
            active: &self.active,
        };
        let _ = slot.entered.send(());
        let _ = slot.release.await;
        drop(active_guard);
        self.completion_order.lock().unwrap().push(key.clone());
        let _ = slot.completed.send(());
        ToolOutcome {
            content: format!("ok:{key}"),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct ActiveGuard<'a> {
    active: &'a AtomicUsize,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ControlledTool {
    name: &'static str,
    permission: PermissionLevel,
    concurrency: ToolConcurrency,
    required_child_depth: u32,
    control: Arc<ToolControl>,
}

#[async_trait]
impl Tool for ControlledTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Deterministic scheduler probe"
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

    fn required_child_depth(&self) -> u32 {
        self.required_child_depth
    }

    fn concurrency(&self) -> ToolConcurrency {
        self.concurrency
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> ToolOutcome {
        self.control
            .execute(
                args["key"]
                    .as_str()
                    .expect("controlled tool key")
                    .to_string(),
            )
            .await
    }
}

fn parallel_tool(
    name: &'static str,
    required_child_depth: u32,
    control: Arc<ToolControl>,
) -> ControlledTool {
    ControlledTool {
        name,
        permission: PermissionLevel::ReadOnly,
        concurrency: ToolConcurrency::ParallelSafe,
        required_child_depth,
        control,
    }
}

fn exclusive_tool(name: &'static str, control: Arc<ToolControl>) -> ControlledTool {
    ControlledTool {
        name,
        permission: PermissionLevel::Execute,
        concurrency: ToolConcurrency::Exclusive,
        required_child_depth: 0,
        control,
    }
}

struct ReleaseGuard {
    slots: Vec<Option<oneshot::Sender<()>>>,
}

impl ReleaseGuard {
    fn from_watches(watches: &mut [ControlWatch]) -> Self {
        Self {
            slots: watches
                .iter_mut()
                .map(|watch| watch.release.take())
                .collect(),
        }
    }

    fn release(&mut self, index: usize) {
        if let Some(sender) = self.slots[index].take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        for sender in &mut self.slots {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
        }
    }
}

#[derive(Default)]
struct RecordingObserver {
    finished: Mutex<Vec<(String, String)>>,
}

impl RecordingObserver {
    fn finished(&self) -> Vec<(String, String)> {
        self.finished.lock().unwrap().clone()
    }
}

impl AgentObserver for RecordingObserver {
    fn on_tool_call_finished(&self, id: &str, outcome: &ToolOutcome) {
        self.finished
            .lock()
            .unwrap()
            .push((id.to_string(), outcome.content.clone()));
    }
}

type AgentRunOutput = (Result<String, ScopedAgentError>, Vec<Message>);

struct AgentRun {
    handle: Option<JoinHandle<AgentRunOutput>>,
}

impl AgentRun {
    fn spawn(agent: Agent, observer: Arc<RecordingObserver>) -> Self {
        let scope = agent.product_root_scope();
        let handle = tokio::spawn(async move {
            let mut history = vec![Message::User("exercise scheduler".to_string())];
            let context = ToolContext {
                cwd: PathBuf::from("."),
                max_output_bytes: 4096,
            };
            let result = agent
                .run_observed_scoped(&scope, &mut history, &context, &NoopSink, observer.as_ref())
                .await;
            (result, history)
        });
        Self {
            handle: Some(handle),
        }
    }

    async fn finish(&mut self) -> AgentRunOutput {
        self.handle
            .take()
            .expect("Agent run handle")
            .await
            .expect("Agent task panicked")
    }
}

impl Drop for AgentRun {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

fn tool_results(messages: &[Message]) -> Vec<(String, String)> {
    messages
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id, content, ..
            } => Some((call_id.clone(), content.clone())),
            _ => None,
        })
        .collect()
}

fn assert_published_in_order(
    expected: &[(String, String)],
    observer: &RecordingObserver,
    history: &[Message],
    provider: &MockProvider,
) {
    assert_eq!(observer.finished(), expected);
    assert_eq!(tool_results(history), expected);
    let requests = provider.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(tool_results(&requests[1].messages), expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn five_delegates_share_four_slots_and_publish_every_occurrence_in_order() {
    let control = Arc::new(ToolControl::default());
    let keys = ["d1", "d2", "d3", "d4", "d5"];
    let mut watches: Vec<_> = keys.iter().map(|key| control.install(key)).collect();
    let mut entered: Vec<_> = watches.iter_mut().map(ControlWatch::take_entered).collect();
    let mut completed: Vec<_> = watches
        .iter_mut()
        .map(ControlWatch::take_completed)
        .collect();
    let mut releases = ReleaseGuard::from_watches(&mut watches);

    let ids = ["duplicate", "call-2", "duplicate", "call-4", "duplicate"];
    let calls = ids
        .iter()
        .zip(keys)
        .map(|(id, key)| call(id, DELEGATE_TASK, key))
        .collect();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(calls),
        final_response("done"),
    ]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(parallel_tool(DELEGATE_TASK, 1, control.clone())))
        .unwrap();
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "mock-model".to_string(),
        4,
    );
    let observer = Arc::new(RecordingObserver::default());
    let mut run = AgentRun::spawn(agent, observer.clone());

    tokio::time::timeout(Duration::from_secs(5), async {
        for receiver in entered.iter_mut().take(4) {
            receiver.await.expect("first four delegates must enter");
        }
        assert_eq!(control.active.load(Ordering::SeqCst), 4);
        assert_eq!(control.max_active.load(Ordering::SeqCst), 4);
        assert!(
            matches!(
                entered[4].try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "fifth delegate must wait while four slots are occupied"
        );

        releases.release(3);
        (&mut completed[3])
            .await
            .expect("fourth delegate completion ack");
        (&mut entered[4])
            .await
            .expect("fifth delegate must enter next");
        assert_eq!(control.active.load(Ordering::SeqCst), 4);

        for index in [2, 1, 4] {
            releases.release(index);
            (&mut completed[index])
                .await
                .unwrap_or_else(|_| panic!("delegate {index} completion ack"));
        }
        assert!(
            observer.finished().is_empty(),
            "out-of-order physical completions must remain buffered behind occurrence 1"
        );

        releases.release(0);
        (&mut completed[0])
            .await
            .expect("first delegate completion ack");
        let (result, history) = run.finish().await;
        assert_eq!(result.expect("Agent run"), "done");
        assert_eq!(
            control.completion_order.lock().unwrap().as_slice(),
            ["d4", "d3", "d2", "d5", "d1"]
        );
        assert_eq!(control.max_active.load(Ordering::SeqCst), 4);

        let expected = ids
            .iter()
            .zip(keys)
            .map(|(id, key)| (id.to_string(), format!("ok:{key}")))
            .collect::<Vec<_>>();
        assert_published_in_order(&expected, &observer, &history, &provider);
        assert_eq!(
            expected.iter().filter(|(id, _)| id == "duplicate").count(),
            3,
            "duplicate call ids are distinct occurrences and must not be deduplicated"
        );
    })
    .await
    .expect("delegate scheduler regression timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_and_delegate_share_one_parallel_segment_before_execute_barrier() {
    let control = Arc::new(ToolControl::default());
    let keys = [
        "read-1",
        "delegate-1",
        "delegate-2",
        "delegate-3",
        "delegate-4",
        "shell",
    ];
    let mut watches: Vec<_> = keys.iter().map(|key| control.install(key)).collect();
    let mut entered: Vec<_> = watches.iter_mut().map(ControlWatch::take_entered).collect();
    let mut completed: Vec<_> = watches
        .iter_mut()
        .map(ControlWatch::take_completed)
        .collect();
    let mut releases = ReleaseGuard::from_watches(&mut watches);

    let calls = vec![
        call("read-call", "read_file", keys[0]),
        call("delegate-call-1", DELEGATE_TASK, keys[1]),
        call("delegate-call-2", DELEGATE_TASK, keys[2]),
        call("delegate-call-3", DELEGATE_TASK, keys[3]),
        call("delegate-call-4", DELEGATE_TASK, keys[4]),
        call("shell-call", "run_shell", keys[5]),
    ];
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(calls),
        final_response("done"),
    ]));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(parallel_tool("read_file", 0, control.clone())))
        .unwrap();
    registry
        .register(Box::new(parallel_tool(DELEGATE_TASK, 1, control.clone())))
        .unwrap();
    registry
        .register(Box::new(exclusive_tool("run_shell", control.clone())))
        .unwrap();
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "mock-model".to_string(),
        4,
    );
    let observer = Arc::new(RecordingObserver::default());
    let mut run = AgentRun::spawn(agent, observer.clone());

    tokio::time::timeout(Duration::from_secs(5), async {
        for receiver in entered.iter_mut().take(4) {
            receiver
                .await
                .expect("first mixed-segment occurrence must enter");
        }
        assert_eq!(control.active.load(Ordering::SeqCst), 4);
        assert_eq!(control.max_active.load(Ordering::SeqCst), 4);
        for (index, label) in [(4, "fifth parallel occurrence"), (5, "run_shell")] {
            assert!(
                matches!(
                    entered[index].try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ),
                "{label} started before the first four released"
            );
        }

        releases.release(3);
        (&mut completed[3])
            .await
            .expect("third delegate completion ack");
        (&mut entered[4])
            .await
            .expect("fourth delegate must enter the shared slot");
        assert_eq!(control.active.load(Ordering::SeqCst), 4);

        for index in [1, 2, 4] {
            releases.release(index);
            (&mut completed[index])
                .await
                .unwrap_or_else(|_| panic!("mixed occurrence {index} completion ack"));
        }
        assert_eq!(control.active.load(Ordering::SeqCst), 1);
        assert!(
            observer.finished().is_empty(),
            "later results must remain buffered while read_file occurrence 1 is pending"
        );
        assert!(
            matches!(
                entered[5].try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "run_shell is a barrier and must wait for the full eligible segment to publish"
        );

        releases.release(0);
        (&mut completed[0]).await.expect("read_file completion ack");
        (&mut entered[5])
            .await
            .expect("run_shell must start after the eligible segment publishes");
        assert_eq!(
            observer
                .finished()
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            [
                "read-call",
                "delegate-call-1",
                "delegate-call-2",
                "delegate-call-3",
                "delegate-call-4",
            ]
        );

        releases.release(5);
        (&mut completed[5]).await.expect("run_shell completion ack");
        let (result, history) = run.finish().await;
        assert_eq!(result.expect("Agent run"), "done");
        assert_eq!(
            control.completion_order.lock().unwrap().as_slice(),
            [
                "delegate-3",
                "delegate-1",
                "delegate-2",
                "delegate-4",
                "read-1",
                "shell",
            ]
        );
        assert_eq!(control.max_active.load(Ordering::SeqCst), 4);

        let expected = [
            ("read-call", "read-1"),
            ("delegate-call-1", "delegate-1"),
            ("delegate-call-2", "delegate-2"),
            ("delegate-call-3", "delegate-3"),
            ("delegate-call-4", "delegate-4"),
            ("shell-call", "shell"),
        ]
        .into_iter()
        .map(|(id, key)| (id.to_string(), format!("ok:{key}")))
        .collect::<Vec<_>>();
        assert_published_in_order(&expected, &observer, &history, &provider);

        // Child Provider futures never call run_blocking_tool. Delegate preflight and
        // filesystem process-limit sharing are locked separately by
        // delegate_preflight_uses_the_process_blocking_limiter_identity and
        // cancelled_scoped_blocking_futures_hold_permits_until_closures_finish (§4.4).
    })
    .await
    .expect("mixed delegate scheduler regression timed out");
}
