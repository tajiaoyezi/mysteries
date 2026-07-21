use super::super::{DelegateTaskTool, WorkspaceCanonicalizer, CHILD_TIMEOUT, DELEGATE_TASK_NAME};
use crate::agent::message::Message;
use crate::agent::{
    Agent, AgentExecutionScope, AgentObserver, AgentRuntime, ExecutionBudget,
    ExecutionCapabilities, ScopedAgentError,
};
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use crate::tool::{
    run_blocking_tool, BlockingToolLimiter, PermissionLevel, ToolContext, ToolOutcome, ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

const TEST_WATCHDOG: Duration = Duration::from_secs(5);
const PARENT_DEADLINE: Duration = Duration::from_secs(30);

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

#[derive(Default)]
struct RecordingObserver {
    started: AtomicUsize,
    finished: AtomicUsize,
    started_signal: Mutex<Option<oneshot::Sender<()>>>,
}

impl RecordingObserver {
    fn with_started_signal(sender: oneshot::Sender<()>) -> Self {
        Self {
            started: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
            started_signal: Mutex::new(Some(sender)),
        }
    }
}

impl AgentObserver for RecordingObserver {
    fn on_tool_call_started(&self, _id: &str, _name: &str, _args: &Value, _readonly: bool) {
        self.started.fetch_add(1, Ordering::SeqCst);
        if let Some(sender) = self.started_signal.lock().unwrap().take() {
            let _ = sender.send(());
        }
    }

    fn on_tool_call_finished(&self, _id: &str, _outcome: &ToolOutcome) {
        self.finished.fetch_add(1, Ordering::SeqCst);
    }
}

struct ReleaseOnDrop {
    sender: Option<mpsc::Sender<()>>,
}

impl ReleaseOnDrop {
    fn new(sender: mpsc::Sender<()>) -> Self {
        Self {
            sender: Some(sender),
        }
    }

    fn release(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

struct StalledCanonicalizer {
    canonicalizer: WorkspaceCanonicalizer,
    entered: Option<oneshot::Receiver<()>>,
    finished: Option<oneshot::Receiver<()>>,
    release: ReleaseOnDrop,
}

impl StalledCanonicalizer {
    fn new() -> Self {
        let (entered_tx, entered_rx) = oneshot::channel();
        let entered_tx = Arc::new(Mutex::new(Some(entered_tx)));
        let (finished_tx, finished_rx) = oneshot::channel();
        let finished_tx = Arc::new(Mutex::new(Some(finished_tx)));
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));

        let canonicalizer = {
            let entered_tx = entered_tx.clone();
            let finished_tx = finished_tx.clone();
            let release_rx = release_rx.clone();
            Arc::new(move |path: &Path| {
                if let Some(sender) = entered_tx.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                let released = release_rx.lock().unwrap().recv_timeout(TEST_WATCHDOG);
                if let Some(sender) = finished_tx.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                released.map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "canonicalizer watchdog fired")
                })?;
                Ok(path.to_path_buf())
            }) as WorkspaceCanonicalizer
        };

        Self {
            canonicalizer,
            entered: Some(entered_rx),
            finished: Some(finished_rx),
            release: ReleaseOnDrop::new(release_tx),
        }
    }

    async fn wait_until_entered(&mut self) {
        self.entered
            .take()
            .expect("canonicalizer entered receiver")
            .await
            .expect("canonicalizer must report entry");
    }

    async fn release_and_wait(&mut self) {
        self.release.release();
        self.finished
            .take()
            .expect("canonicalizer finished receiver")
            .await
            .expect("canonicalizer must report completion");
    }
}

struct PermitHolder {
    release: ReleaseOnDrop,
    handle: Option<JoinHandle<ToolOutcome>>,
}

impl PermitHolder {
    async fn start(limiter: BlockingToolLimiter) -> Self {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let handle = tokio::spawn(async move {
            run_blocking_tool(&limiter, move || {
                let _ = entered_tx.send(());
                let released = release_rx.lock().unwrap().recv_timeout(TEST_WATCHDOG);
                ToolOutcome {
                    content: if released.is_ok() {
                        "permit released".to_string()
                    } else {
                        "permit watchdog fired".to_string()
                    },
                    is_error: released.is_err(),
                    truncated: false,
                    exit: None,
                }
            })
            .await
        });
        entered_rx.await.expect("permit holder must enter");

        Self {
            release: ReleaseOnDrop::new(release_tx),
            handle: Some(handle),
        }
    }

    async fn release_and_wait(&mut self) {
        self.release.release();
        let outcome = self
            .handle
            .take()
            .expect("permit holder task")
            .await
            .expect("permit holder join");
        assert!(!outcome.is_error, "permit holder failed: {outcome:?}");
    }
}

impl Drop for PermitHolder {
    fn drop(&mut self) {
        self.release.release();
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

type RunResult = (Result<String, ScopedAgentError>, Vec<Message>);

struct ParentRun {
    handle: Option<JoinHandle<RunResult>>,
}

impl ParentRun {
    fn spawn(
        agent: Agent,
        scope: AgentExecutionScope,
        context: ToolContext,
        observer: Arc<dyn AgentObserver>,
    ) -> Self {
        let handle = tokio::spawn(async move {
            let mut history = vec![Message::User("delegate now".to_string())];
            let sink = NoopSink;
            let result = agent
                .run_observed_scoped(&scope, &mut history, &context, &sink, observer.as_ref())
                .await;
            (result, history)
        });
        Self {
            handle: Some(handle),
        }
    }

    async fn finish(&mut self) -> RunResult {
        let handle = self.handle.as_mut().expect("parent run task");
        let joined = tokio::select! {
            result = handle => result,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("parent run did not finish after scope termination")
            }
        };
        self.handle.take();
        joined.expect("parent run join")
    }
}

impl Drop for ParentRun {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

fn tool_response() -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: "delegate-deadline".to_string(),
            name: DELEGATE_TASK_NAME.to_string(),
            arguments: json!({ "task": "inspect the workspace" }),
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
        thinking: Vec::new(),
    }
}

fn final_response() -> ModelResponse {
    ModelResponse {
        text: "parent recovered".to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: None,
        thinking: Vec::new(),
    }
}

fn parent_scope(deadline: Option<Instant>) -> AgentExecutionScope {
    AgentExecutionScope::root(
        ExecutionBudget::new(3, deadline, 1),
        ExecutionCapabilities::try_new([DELEGATE_TASK_NAME], [PermissionLevel::ReadOnly]).unwrap(),
    )
}

fn parent_agent(
    canonicalizer: WorkspaceCanonicalizer,
    limiter: BlockingToolLimiter,
    outer_script: Vec<ModelResponse>,
) -> (Agent, Arc<MockProvider>, Arc<MockProvider>) {
    let child_provider = Arc::new(MockProvider::new(Vec::new()));
    let child_runtime = AgentRuntime::new(child_provider.clone(), "child-model".to_string());
    let delegate = DelegateTaskTool::with_dependencies(
        child_runtime,
        ToolRegistry::new(),
        canonicalizer,
        limiter,
    );
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(delegate)).unwrap();

    let outer_provider = Arc::new(MockProvider::new(outer_script));
    let agent = Agent::new(
        outer_provider.clone(),
        registry,
        Box::new(DenyAll),
        "parent-model".to_string(),
        3,
    );
    (agent, outer_provider, child_provider)
}

fn tool_context(cwd: impl Into<PathBuf>) -> ToolContext {
    ToolContext {
        cwd: cwd.into(),
        max_output_bytes: 4096,
    }
}

fn tool_results(history: &[Message]) -> Vec<(&str, &str, bool)> {
    history
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => Some((call_id.as_str(), content.as_str(), *is_error)),
            _ => None,
        })
        .collect()
}

fn assert_child_deadline_outcome(result: RunResult, observer: &RecordingObserver) {
    let (run_result, history) = result;
    assert_eq!(run_result, Ok("parent recovered".to_string()));
    let results = tool_results(&history);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "delegate-deadline");
    assert!(results[0].2);
    assert_eq!(observer.started.load(Ordering::SeqCst), 1);
    assert_eq!(observer.finished.load(Ordering::SeqCst), 1);
    assert!(
        results[0].1.starts_with("delegate_task failed:"),
        "child-only deadline must use the ordinary delegate error envelope: {}",
        results[0].1
    );
    assert!(
        results[0].1.to_ascii_lowercase().contains("deadline"),
        "child-only deadline reason must remain visible: {}",
        results[0].1
    );
}

#[tokio::test(start_paused = true)]
async fn child_deadline_covers_blocking_permit_wait_and_parent_continues() {
    let workspace = tempfile::tempdir().unwrap();
    let limiter = BlockingToolLimiter::new(1);
    let mut permit_holder = PermitHolder::start(limiter.clone()).await;
    let canonicalizer_calls = Arc::new(AtomicUsize::new(0));
    let canonicalizer = {
        let calls = canonicalizer_calls.clone();
        Arc::new(move |path: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(path.to_path_buf())
        }) as WorkspaceCanonicalizer
    };
    let (agent, outer_provider, child_provider) = parent_agent(
        canonicalizer,
        limiter,
        vec![tool_response(), final_response()],
    );
    let scope = parent_scope(None);
    let (started_tx, started_rx) = oneshot::channel();
    let observer = Arc::new(RecordingObserver::with_started_signal(started_tx));
    let mut run = ParentRun::spawn(
        agent,
        scope,
        tool_context(workspace.path()),
        observer.clone(),
    );

    started_rx
        .await
        .expect("delegate outer occurrence must start");
    tokio::task::yield_now().await;
    assert_eq!(canonicalizer_calls.load(Ordering::SeqCst), 0);

    tokio::time::advance(CHILD_TIMEOUT).await;
    let result = run.finish().await;

    permit_holder.release_and_wait().await;
    tokio::task::yield_now().await;
    assert_eq!(
        canonicalizer_calls.load(Ordering::SeqCst),
        0,
        "timed-out permit waiter must not start preflight after a permit is released"
    );
    assert!(child_provider.recorded_requests().is_empty());
    assert_eq!(outer_provider.recorded_requests().len(), 2);
    assert_child_deadline_outcome(result, observer.as_ref());
}

#[tokio::test(start_paused = true)]
async fn child_deadline_covers_stalled_preflight_and_late_release_cannot_start_child() {
    let workspace = tempfile::tempdir().unwrap();
    let mut stall = StalledCanonicalizer::new();
    let (agent, outer_provider, child_provider) = parent_agent(
        stall.canonicalizer.clone(),
        BlockingToolLimiter::new(1),
        vec![tool_response(), final_response()],
    );
    let scope = parent_scope(None);
    let observer = Arc::new(RecordingObserver::default());
    let mut run = ParentRun::spawn(
        agent,
        scope,
        tool_context(workspace.path()),
        observer.clone(),
    );

    stall.wait_until_entered().await;
    tokio::time::advance(CHILD_TIMEOUT).await;
    let result = run.finish().await;

    assert!(child_provider.recorded_requests().is_empty());
    stall.release_and_wait().await;
    tokio::task::yield_now().await;
    assert!(
        child_provider.recorded_requests().is_empty(),
        "late canonicalizer completion must not construct or run the child"
    );
    assert_eq!(outer_provider.recorded_requests().len(), 2);
    assert_child_deadline_outcome(result, observer.as_ref());
}

#[derive(Clone, Copy)]
enum ParentTermination {
    Cancelled,
    Deadline,
}

async fn assert_parent_termination_is_single_synthetic(termination: ParentTermination) {
    let workspace = tempfile::tempdir().unwrap();
    let mut stall = StalledCanonicalizer::new();
    let (agent, outer_provider, child_provider) = parent_agent(
        stall.canonicalizer.clone(),
        BlockingToolLimiter::new(1),
        vec![tool_response()],
    );
    let deadline = matches!(termination, ParentTermination::Deadline)
        .then(|| Instant::now() + PARENT_DEADLINE);
    let scope = parent_scope(deadline);
    let observer = Arc::new(RecordingObserver::default());
    let mut run = ParentRun::spawn(
        agent,
        scope.clone(),
        tool_context(workspace.path()),
        observer.clone(),
    );

    stall.wait_until_entered().await;
    let (expected_error, expected_content) = match termination {
        ParentTermination::Cancelled => {
            scope.cancel();
            (
                ScopedAgentError::Cancelled,
                "tool call interrupted before completion",
            )
        }
        ParentTermination::Deadline => {
            tokio::time::advance(PARENT_DEADLINE).await;
            (
                ScopedAgentError::DeadlineExceeded,
                "tool call deadline exceeded before completion",
            )
        }
    };
    let (run_result, history) = run.finish().await;

    assert_eq!(run_result, Err(expected_error));
    assert_eq!(
        tool_results(&history),
        vec![("delegate-deadline", expected_content, true)]
    );
    assert_eq!(observer.started.load(Ordering::SeqCst), 1);
    assert_eq!(
        observer.finished.load(Ordering::SeqCst),
        0,
        "synthetic parent termination must not publish an ordinary tool outcome"
    );
    assert!(child_provider.recorded_requests().is_empty());
    assert_eq!(outer_provider.recorded_requests().len(), 1);

    stall.release_and_wait().await;
    tokio::task::yield_now().await;
    assert!(child_provider.recorded_requests().is_empty());
    assert_eq!(
        observer.finished.load(Ordering::SeqCst),
        0,
        "late preflight completion must not publish a duplicate ordinary outcome"
    );
}

#[tokio::test(start_paused = true)]
async fn earlier_parent_cancel_and_deadline_publish_only_one_synthetic_result() {
    assert_parent_termination_is_single_synthetic(ParentTermination::Cancelled).await;
    assert_parent_termination_is_single_synthetic(ParentTermination::Deadline).await;
}
