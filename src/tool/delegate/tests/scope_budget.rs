use super::super::{
    run_scoped_blocking, DelegateTaskTool, CHILD_MAX_ITERATIONS, CHILD_TIMEOUT, DELEGATE_TASK_NAME,
};
use crate::agent::message::Message;
use crate::agent::{
    Agent, AgentExecutionScope, AgentObserver, AgentRuntime, ExecutionBudget,
    ExecutionCapabilities, NoopObserver,
};
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use crate::tool::{
    process_blocking_limiter, run_blocking_tool, BlockingToolLimiter, PermissionLevel, Tool,
    ToolConcurrency, ToolContext, ToolExecutionContext, ToolOutcome, ToolRegistry,
    MAX_BLOCKING_TOOL_CALLS,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::Instant;

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

#[derive(Default)]
struct CountingAllow {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl PermissionDecider for CountingAllow {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PermissionDecision::Allow
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

fn tool_response(name: &str) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments: json!({}),
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
        thinking: Vec::new(),
    }
}

fn tool_context(cwd: impl Into<PathBuf>) -> ToolContext {
    ToolContext {
        cwd: cwd.into(),
        max_output_bytes: 4096,
    }
}

#[tokio::test]
async fn product_root_exposes_delegate_and_derives_a_depth_zero_child() {
    let provider = Arc::new(MockProvider::new(vec![final_response("done")]));
    let runtime = AgentRuntime::new(provider.clone(), "parent-model".to_string());
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(DelegateTaskTool::new(
            runtime.clone(),
            ToolRegistry::new(),
        )))
        .unwrap();
    let agent = Agent::with_runtime(runtime, registry, Box::new(CountingAllow::default()), 20);
    let root = agent.product_root_scope();
    let mut history = vec![Message::User("inspect".to_string())];

    agent
        .run_observed_scoped(
            &root,
            &mut history,
            &tool_context("."),
            &NoopSink,
            &NoopObserver,
        )
        .await
        .unwrap();

    assert_eq!(root.budget().remaining_child_depth, 1);
    assert!(
        provider.recorded_requests()[0]
            .tools
            .iter()
            .any(|schema| schema.name == DELEGATE_TASK_NAME),
        "depth=1 product root must expose delegate_task"
    );

    let child = root
        .derive_child(
            ExecutionBudget::new(
                CHILD_MAX_ITERATIONS,
                Some(Instant::now() + CHILD_TIMEOUT),
                0,
            ),
            ExecutionCapabilities::try_new([DELEGATE_TASK_NAME], [PermissionLevel::ReadOnly])
                .unwrap(),
        )
        .unwrap();
    assert_eq!(child.budget().remaining_child_depth, 0);
    assert_eq!(
        child.identity().parent_run_id(),
        Some(root.identity().run_id())
    );
}

struct DepthProbe {
    executes: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for DepthProbe {
    fn name(&self) -> &str {
        DELEGATE_TASK_NAME
    }

    fn description(&self) -> &str {
        "depth probe"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn required_child_depth(&self) -> u32 {
        1
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        self.executes.fetch_add(1, Ordering::SeqCst);
        ToolOutcome {
            content: "must not execute".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

#[derive(Default)]
struct StartedObserver {
    started: AtomicUsize,
}

impl AgentObserver for StartedObserver {
    fn on_tool_call_started(&self, _id: &str, _name: &str, _args: &Value, _readonly: bool) {
        self.started.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn legacy_depth_zero_hides_delegate_and_rejects_hard_calls_before_side_effects() {
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(DELEGATE_TASK_NAME),
        final_response("recovered"),
    ]));
    let executes = Arc::new(AtomicUsize::new(0));
    let permission_calls = Arc::new(AtomicUsize::new(0));
    let decider = CountingAllow {
        calls: permission_calls.clone(),
    };
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(DepthProbe {
            executes: executes.clone(),
        }))
        .unwrap();
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(decider),
        "legacy-model".to_string(),
        2,
    );
    let root = agent.root_scope();
    let observer = StartedObserver::default();
    let mut history = vec![Message::User("hard call delegate".to_string())];

    agent
        .run_observed_scoped(
            &root,
            &mut history,
            &tool_context("."),
            &NoopSink,
            &observer,
        )
        .await
        .unwrap();

    assert_eq!(root.budget().remaining_child_depth, 0);
    assert!(
        provider.recorded_requests()[0]
            .tools
            .iter()
            .all(|schema| schema.name != DELEGATE_TASK_NAME),
        "legacy depth=0 schema must omit delegate_task"
    );
    assert_eq!(
        observer.started.load(Ordering::SeqCst),
        0,
        "depth rejection must happen before observer start"
    );
    assert_eq!(
        permission_calls.load(Ordering::SeqCst),
        0,
        "depth rejection must happen before permission"
    );
    assert_eq!(
        executes.load(Ordering::SeqCst),
        0,
        "depth rejection must happen before execute"
    );
}

struct BudgetProbe {
    budget_tx: Mutex<Option<oneshot::Sender<ExecutionBudget>>>,
}

#[async_trait]
impl Tool for BudgetProbe {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "captures child budget"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> ToolConcurrency {
        ToolConcurrency::ParallelSafe
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        panic!("child must use scoped dispatch")
    }

    async fn execute_scoped(&self, _args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        if let Some(tx) = self.budget_tx.lock().unwrap().take() {
            let _ = tx.send(*ctx.scope.budget());
        }
        ToolOutcome {
            content: "captured".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct ReleaseOnDrop {
    tx: Option<mpsc::Sender<()>>,
}

impl ReleaseOnDrop {
    fn release(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

async fn assert_budget_captured_before_preflight(
    parent_max_iterations: u32,
    parent_deadline_after: Option<Duration>,
    expected_max_iterations: u32,
    expected_deadline_after: Duration,
) {
    let invocation_time = Instant::now();
    let old_provider = Arc::new(MockProvider::new(vec![
        tool_response("list_dir"),
        final_response("child done"),
    ]));
    let new_provider = Arc::new(MockProvider::new(Vec::new()));
    let runtime = AgentRuntime::new(old_provider.clone(), "old-model".to_string());
    let (budget_tx, budget_rx) = oneshot::channel();
    let mut child_registry = ToolRegistry::new();
    child_registry
        .register(Box::new(BudgetProbe {
            budget_tx: Mutex::new(Some(budget_tx)),
        }))
        .unwrap();

    let (entered_tx, entered_rx) = oneshot::channel();
    let entered_tx = Arc::new(Mutex::new(Some(entered_tx)));
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let canonicalizer = {
        let entered_tx = entered_tx.clone();
        let release_rx = release_rx.clone();
        Arc::new(move |path: &Path| {
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            let _ = release_rx.lock().unwrap().recv();
            Ok(path.to_path_buf())
        })
    };
    let tool = DelegateTaskTool::with_dependencies(
        runtime.clone(),
        child_registry,
        canonicalizer,
        BlockingToolLimiter::new(1),
    );
    let parent_deadline = parent_deadline_after.map(|duration| invocation_time + duration);
    let parent = AgentExecutionScope::root(
        ExecutionBudget::new(parent_max_iterations, parent_deadline, 1),
        ExecutionCapabilities::try_new(
            [DELEGATE_TASK_NAME, "list_dir"],
            [PermissionLevel::ReadOnly],
        )
        .unwrap(),
    );
    let temp = tempfile::tempdir().unwrap();
    let tool_context = tool_context(temp.path());
    let observer = NoopObserver;
    let execution_context = ToolExecutionContext {
        tool: &tool_context,
        scope: &parent,
        observer: &observer,
        read_root: None,
    };
    let mut release = ReleaseOnDrop {
        tx: Some(release_tx),
    };
    let mut execution =
        Box::pin(tool.execute_scoped(json!({ "task": "preserve exactly" }), &execution_context));

    tokio::select! {
        entered = entered_rx => entered.expect("canonicalizer entered sender"),
        outcome = &mut execution => panic!("valid delegate returned before preflight: {outcome:?}"),
    }

    runtime.replace_provider_model(new_provider.clone(), "new-model".to_string());
    tokio::time::advance(Duration::from_secs(30)).await;
    release.release();

    let outcome = tokio::time::timeout(Duration::from_secs(1), &mut execution)
        .await
        .expect("delegate should finish after preflight release");
    assert!(
        !outcome.is_error,
        "delegate failed unexpectedly: {outcome:?}"
    );
    let budget = budget_rx.await.expect("child budget probe must execute");
    assert_eq!(budget.max_iterations, expected_max_iterations);
    assert_eq!(budget.remaining_child_depth, 0);
    assert_eq!(
        budget.deadline,
        Some(invocation_time + expected_deadline_after),
        "deadline must be captured before the 30s preflight stall"
    );
    assert_eq!(old_provider.recorded_requests().len(), 2);
    assert_eq!(old_provider.recorded_requests()[0].model, "old-model");
    assert!(
        new_provider.recorded_requests().is_empty(),
        "an invocation must keep the frozen preflight snapshot"
    );
}

#[tokio::test(start_paused = true)]
async fn delegate_budget_clamps_parent_and_starts_deadline_before_preflight() {
    assert_budget_captured_before_preflight(4, None, 4, CHILD_TIMEOUT).await;
    assert_budget_captured_before_preflight(
        20,
        Some(Duration::from_secs(60)),
        CHILD_MAX_ITERATIONS,
        Duration::from_secs(60),
    )
    .await;
}

#[test]
fn delegate_preflight_uses_the_process_blocking_limiter_identity() {
    let runtime = AgentRuntime::new(Arc::new(MockProvider::new(Vec::new())), "model".to_string());
    let delegate = DelegateTaskTool::new(runtime, ToolRegistry::new());
    let process = process_blocking_limiter();

    assert!(
        Arc::ptr_eq(&delegate.limiter.semaphore, &process.semaphore),
        "delegate preflight and filesystem tools must share the process limiter"
    );
}

struct MultiReleaseOnDrop {
    tx: mpsc::Sender<()>,
}

impl MultiReleaseOnDrop {
    fn release(&self, count: usize) {
        for _ in 0..count {
            let _ = self.tx.send(());
        }
    }
}

impl Drop for MultiReleaseOnDrop {
    fn drop(&mut self) {
        self.release(MAX_BLOCKING_TOOL_CALLS * 2);
    }
}

fn enter_and_wait(
    active: &AtomicUsize,
    max_active: &AtomicUsize,
    entered: &mpsc::Sender<()>,
    release: &Mutex<mpsc::Receiver<()>>,
    done: &mpsc::Sender<()>,
) -> usize {
    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
    max_active.fetch_max(now, Ordering::SeqCst);
    let _ = entered.send(());
    let _ = release.lock().unwrap().recv();
    active.fetch_sub(1, Ordering::SeqCst);
    let _ = done.send(());
    now
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_scoped_blocking_futures_hold_permits_until_closures_finish() {
    let limiter = BlockingToolLimiter::new(MAX_BLOCKING_TOOL_CALLS);
    let scope = AgentExecutionScope::root(
        ExecutionBudget::new(8, None, 0),
        ExecutionCapabilities::try_new(
            std::iter::empty::<&str>(),
            std::iter::empty::<PermissionLevel>(),
        )
        .unwrap(),
    );
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let (first_entered_tx, first_entered_rx) = mpsc::channel();
    let (second_entered_tx, second_entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = MultiReleaseOnDrop {
        tx: release_tx.clone(),
    };
    let release_rx = Arc::new(Mutex::new(release_rx));
    let (done_tx, done_rx) = mpsc::channel();

    let mut first = Vec::new();
    for _ in 0..MAX_BLOCKING_TOOL_CALLS {
        let limiter = limiter.clone();
        let scope = scope.clone();
        let active = active.clone();
        let max_active = max_active.clone();
        let entered = first_entered_tx.clone();
        let release_rx = release_rx.clone();
        let done = done_tx.clone();
        first.push(tokio::spawn(async move {
            run_scoped_blocking(&scope, &limiter, move || {
                enter_and_wait(&active, &max_active, &entered, &release_rx, &done)
            })
            .await
        }));
    }
    for _ in 0..MAX_BLOCKING_TOOL_CALLS {
        first_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first scoped blocking batch should enter");
    }
    for handle in first {
        handle.abort();
        assert!(handle
            .await
            .expect_err("first awaiting future must cancel")
            .is_cancelled());
    }
    assert_eq!(limiter.semaphore.available_permits(), 0);

    let (queued_tx, mut queued_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut second = Vec::new();
    for _ in 0..(MAX_BLOCKING_TOOL_CALLS - 1) {
        let limiter = limiter.clone();
        let scope = scope.clone();
        let active = active.clone();
        let max_active = max_active.clone();
        let entered = second_entered_tx.clone();
        let release_rx = release_rx.clone();
        let done = done_tx.clone();
        let queued = queued_tx.clone();
        second.push(tokio::spawn(async move {
            let _ = queued.send(());
            run_scoped_blocking(&scope, &limiter, move || {
                enter_and_wait(&active, &max_active, &entered, &release_rx, &done)
            })
            .await
            .map(|_| ())
        }));
    }
    let limiter_for_fs = limiter.clone();
    let active_for_fs = active.clone();
    let max_for_fs = max_active.clone();
    let entered_for_fs = second_entered_tx.clone();
    let release_for_fs = release_rx.clone();
    let done_for_fs = done_tx.clone();
    let queued_for_fs = queued_tx.clone();
    let fs = tokio::spawn(async move {
        let _ = queued_for_fs.send(());
        run_blocking_tool(&limiter_for_fs, move || {
            enter_and_wait(
                &active_for_fs,
                &max_for_fs,
                &entered_for_fs,
                &release_for_fs,
                &done_for_fs,
            );
            ToolOutcome {
                content: "fs".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        })
        .await
    });
    drop(queued_tx);
    for _ in 0..MAX_BLOCKING_TOOL_CALLS {
        queued_rx.recv().await.expect("second batch queued");
    }
    assert!(matches!(
        second_entered_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    release.release(MAX_BLOCKING_TOOL_CALLS);
    for _ in 0..MAX_BLOCKING_TOOL_CALLS {
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first detached closures should finish");
    }
    for _ in 0..MAX_BLOCKING_TOOL_CALLS {
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second batch should enter only after first release");
    }
    release.release(MAX_BLOCKING_TOOL_CALLS);
    for handle in second {
        handle
            .await
            .expect("second task join")
            .expect("scoped result");
    }
    assert!(!fs.await.expect("fs task join").is_error);
    assert!(
        max_active.load(Ordering::SeqCst) <= MAX_BLOCKING_TOOL_CALLS,
        "global blocking max exceeded: {}",
        max_active.load(Ordering::SeqCst)
    );
}
