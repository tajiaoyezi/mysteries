use super::message::Message;
use super::{
    Agent, AgentExecutionScope, AgentObserver, AgentStatus, ContextError, ContextStrategy,
    ExecutionBudget, ExecutionCapabilities, RunIdentity, ScopedAgentError,
};
use crate::error::ProviderError;
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision, PermissionMode};
use crate::provider::mock::MockProvider;
use crate::provider::{
    DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall, Usage,
};
use crate::tool::{
    run_blocking_tool, BlockingToolLimiter, PermissionLevel, Tool, ToolConcurrency, ToolContext,
    ToolExecutionContext, ToolOutcome, ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::future::pending;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::Instant;

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

fn ctx() -> ToolContext {
    ToolContext {
        cwd: PathBuf::from("."),
        max_output_bytes: 4096,
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

fn response_with_usage(text: &str) -> ModelResponse {
    ModelResponse {
        text: text.to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: Some(Usage {
            input_tokens: 2,
            output_tokens: 3,
        }),
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

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: json!({}),
    }
}

fn scope(
    tool_names: &[&str],
    permission_levels: &[PermissionLevel],
    max_iterations: u32,
    deadline: Option<Instant>,
) -> AgentExecutionScope {
    AgentExecutionScope::root(
        ExecutionBudget::new(max_iterations, deadline, 0),
        ExecutionCapabilities::try_new(
            tool_names.iter().copied(),
            permission_levels.iter().cloned(),
        )
        .unwrap(),
    )
}

struct ImmediateTool {
    name: &'static str,
}

#[async_trait]
impl Tool for ImmediateTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Immediate read-only tool"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        ToolOutcome {
            content: format!("ok:{}", self.name),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

fn registry_with_immediate(name: &'static str) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ImmediateTool { name })).unwrap();
    registry
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScopedContextRecord {
    identity: RunIdentity,
    observer_address: usize,
    cwd: PathBuf,
    read_root: Option<PathBuf>,
}

#[derive(Default)]
struct ScopedContextProbeState {
    legacy_calls: AtomicUsize,
    scoped_calls: AtomicUsize,
    records: Mutex<Vec<ScopedContextRecord>>,
}

struct ScopedContextProbeTool {
    concurrency: ToolConcurrency,
    state: Arc<ScopedContextProbeState>,
}

#[async_trait]
impl Tool for ScopedContextProbeTool {
    fn name(&self) -> &str {
        "scoped_context_probe"
    }

    fn description(&self) -> &str {
        "Records the scoped execution context."
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> ToolConcurrency {
        self.concurrency
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        self.state.legacy_calls.fetch_add(1, Ordering::SeqCst);
        ToolOutcome {
            content: "legacy execute".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }

    async fn execute_scoped(&self, _args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        self.state.scoped_calls.fetch_add(1, Ordering::SeqCst);
        self.state
            .records
            .lock()
            .unwrap()
            .push(ScopedContextRecord {
                identity: ctx.scope.identity(),
                observer_address: ctx.observer as *const dyn AgentObserver as *const () as usize,
                cwd: ctx.tool.cwd.clone(),
                read_root: ctx.read_root.map(PathBuf::from),
            });
        ToolOutcome {
            content: "scoped execute".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

fn registry_with_scoped_probe(
    concurrency: ToolConcurrency,
    state: Arc<ScopedContextProbeState>,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(ScopedContextProbeTool { concurrency, state }))
        .unwrap();
    registry
}

struct PendingProvider {
    entered: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl Provider for PendingProvider {
    fn name(&self) -> &str {
        "pending"
    }

    async fn complete(
        &self,
        _req: ModelRequest,
        _sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        pending().await
    }
}

struct PendingStrategy {
    entered: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl ContextStrategy for PendingStrategy {
    async fn prepare(
        &self,
        _history: &[Message],
        _last_usage: Option<&Usage>,
    ) -> Result<Vec<Message>, ContextError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        pending().await
    }
}

struct PendingDecider {
    entered: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl PermissionDecider for PendingDecider {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        pending().await
    }
}

struct ExecuteTool {
    executions: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for ExecuteTool {
    fn name(&self) -> &str {
        "execute"
    }

    fn description(&self) -> &str {
        "Permission-controlled tool"
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
            content: "executed".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct ControlledWatch {
    entered: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    completed: oneshot::Receiver<()>,
}

struct ControlledTool {
    name: &'static str,
    concurrency: ToolConcurrency,
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    completed: Mutex<Option<oneshot::Sender<()>>>,
}

impl ControlledTool {
    fn pair(name: &'static str, concurrency: ToolConcurrency) -> (Self, ControlledWatch) {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let (completed_tx, completed_rx) = oneshot::channel();
        (
            Self {
                name,
                concurrency,
                entered: Mutex::new(Some(entered_tx)),
                release: Mutex::new(Some(release_rx)),
                completed: Mutex::new(Some(completed_tx)),
            },
            ControlledWatch {
                entered: entered_rx,
                release: Some(release_tx),
                completed: completed_rx,
            },
        )
    }
}

#[async_trait]
impl Tool for ControlledTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Controlled tool"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> ToolConcurrency {
        self.concurrency
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        let release = self.release.lock().unwrap().take().unwrap();
        let _ = release.await;
        if let Some(completed) = self.completed.lock().unwrap().take() {
            let _ = completed.send(());
        }
        ToolOutcome {
            content: format!("ok:{}", self.name),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct BlockingControlledTool {
    entered: Mutex<Option<oneshot::Sender<()>>>,
    completed: Mutex<Option<oneshot::Sender<()>>>,
    release: Arc<(Mutex<bool>, Condvar)>,
    limiter: BlockingToolLimiter,
}

#[async_trait]
impl Tool for BlockingControlledTool {
    fn name(&self) -> &str {
        "blocking"
    }

    fn description(&self) -> &str {
        "Controlled blocking tool"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        let entered = self.entered.lock().unwrap().take();
        let completed = self.completed.lock().unwrap().take();
        let release = self.release.clone();
        run_blocking_tool(&self.limiter, move || {
            if let Some(entered) = entered {
                let _ = entered.send(());
            }
            let (released, wake) = &*release;
            let mut released = released.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
            if let Some(completed) = completed {
                let _ = completed.send(());
            }
            ToolOutcome {
                content: "late blocking result".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            }
        })
        .await
    }
}

#[derive(Default)]
struct IdentityObserver {
    identities: Mutex<Vec<RunIdentity>>,
    legacy_status_calls: AtomicUsize,
}

impl AgentObserver for IdentityObserver {
    fn on_status(&self, _status: AgentStatus) {
        self.legacy_status_calls.fetch_add(1, Ordering::SeqCst);
    }

    fn on_scoped_status(&self, identity: &RunIdentity, _status: AgentStatus) {
        self.identities.lock().unwrap().push(*identity);
    }

    fn on_scoped_usage(&self, identity: &RunIdentity, _usage: &Usage) {
        self.identities.lock().unwrap().push(*identity);
    }
}

struct FinishObserver {
    finished: Mutex<Vec<(String, ToolOutcome)>>,
    first_finished: Mutex<Option<oneshot::Sender<()>>>,
}

impl FinishObserver {
    fn new(first_finished: oneshot::Sender<()>) -> Self {
        Self {
            finished: Mutex::new(Vec::new()),
            first_finished: Mutex::new(Some(first_finished)),
        }
    }

    fn record(&self, id: &str, outcome: &ToolOutcome) {
        self.finished
            .lock()
            .unwrap()
            .push((id.to_string(), outcome.clone()));
        if let Some(signal) = self.first_finished.lock().unwrap().take() {
            let _ = signal.send(());
        }
    }
}

impl AgentObserver for FinishObserver {
    fn on_tool_call_finished(&self, id: &str, outcome: &ToolOutcome) {
        self.record(id, outcome);
    }

    fn on_scoped_tool_call_finished(
        &self,
        _identity: &RunIdentity,
        id: &str,
        outcome: &ToolOutcome,
    ) {
        self.record(id, outcome);
    }
}

fn tool_results(history: &[Message]) -> Vec<(String, String, bool)> {
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

fn observer_address(observer: &dyn AgentObserver) -> usize {
    observer as *const dyn AgentObserver as *const () as usize
}

#[tokio::test]
async fn serial_dispatch_passes_current_scoped_tool_context() {
    let state = Arc::new(ScopedContextProbeState::default());
    let registry = registry_with_scoped_probe(ToolConcurrency::Exclusive, state.clone());
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("serial", "scoped_context_probe")]),
        response("done"),
    ]));
    let agent = Agent::new(
        provider,
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        2,
    )
    .with_read_root(PathBuf::from("serial-read-root"));
    let scope = scope(
        &["scoped_context_probe"],
        &[PermissionLevel::ReadOnly],
        2,
        None,
    );
    let observer = IdentityObserver::default();
    let context = ToolContext {
        cwd: PathBuf::from("serial-cwd"),
        max_output_bytes: 4096,
    };
    let mut history = vec![Message::User("serial".to_string())];

    agent
        .run_observed_scoped(&scope, &mut history, &context, &NoopSink, &observer)
        .await
        .unwrap();

    assert_eq!(
        state.scoped_calls.load(Ordering::SeqCst),
        1,
        "scoped override未被调用：Agent serial dispatch仍走legacy execute"
    );
    assert_eq!(state.legacy_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        *state.records.lock().unwrap(),
        vec![ScopedContextRecord {
            identity: scope.identity(),
            observer_address: observer_address(&observer),
            cwd: PathBuf::from("serial-cwd"),
            read_root: Some(PathBuf::from("serial-read-root")),
        }]
    );
}

#[tokio::test]
async fn parallel_dispatch_passes_current_scoped_tool_context_per_occurrence() {
    let state = Arc::new(ScopedContextProbeState::default());
    let registry = registry_with_scoped_probe(ToolConcurrency::ParallelSafe, state.clone());
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![
            call("parallel-1", "scoped_context_probe"),
            call("parallel-2", "scoped_context_probe"),
        ]),
        response("done"),
    ]));
    let agent = Agent::new(
        provider,
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        2,
    )
    .with_read_root(PathBuf::from("parallel-read-root"));
    let scope = scope(
        &["scoped_context_probe"],
        &[PermissionLevel::ReadOnly],
        2,
        None,
    );
    let observer = IdentityObserver::default();
    let context = ToolContext {
        cwd: PathBuf::from("parallel-cwd"),
        max_output_bytes: 4096,
    };
    let mut history = vec![Message::User("parallel".to_string())];

    agent
        .run_observed_scoped(&scope, &mut history, &context, &NoopSink, &observer)
        .await
        .unwrap();

    assert_eq!(
        state.scoped_calls.load(Ordering::SeqCst),
        2,
        "scoped override未被调用：Agent ParallelSafe dispatch仍走legacy execute"
    );
    assert_eq!(state.legacy_calls.load(Ordering::SeqCst), 0);
    let records = state.records.lock().unwrap();
    assert_eq!(records.len(), 2);
    for record in records.iter() {
        assert_eq!(record.identity, scope.identity());
        assert_eq!(record.observer_address, observer_address(&observer));
        assert_eq!(record.cwd, PathBuf::from("parallel-cwd"));
        assert_eq!(record.read_root, Some(PathBuf::from("parallel-read-root")));
    }
}

#[tokio::test]
async fn concurrent_runs_sharing_one_tool_do_not_cross_scoped_context() {
    let state = Arc::new(ScopedContextProbeState::default());
    let registry = registry_with_scoped_probe(ToolConcurrency::Exclusive, state.clone());
    let first_agent = Agent::new(
        Arc::new(MockProvider::new(vec![
            tool_response(vec![call("first", "scoped_context_probe")]),
            response("first done"),
        ])),
        registry.clone(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    )
    .with_read_root(PathBuf::from("first-read-root"));
    let second_agent = Agent::new(
        Arc::new(MockProvider::new(vec![
            tool_response(vec![call("second", "scoped_context_probe")]),
            response("second done"),
        ])),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        2,
    )
    .with_read_root(PathBuf::from("second-read-root"));
    let first_scope = scope(
        &["scoped_context_probe"],
        &[PermissionLevel::ReadOnly],
        2,
        None,
    );
    let second_scope = scope(
        &["scoped_context_probe"],
        &[PermissionLevel::ReadOnly],
        2,
        None,
    );
    let first_observer = IdentityObserver::default();
    let second_observer = IdentityObserver::default();
    let first_context = ToolContext {
        cwd: PathBuf::from("first-cwd"),
        max_output_bytes: 4096,
    };
    let second_context = ToolContext {
        cwd: PathBuf::from("second-cwd"),
        max_output_bytes: 4096,
    };
    let mut first_history = vec![Message::User("first".to_string())];
    let mut second_history = vec![Message::User("second".to_string())];

    let (first, second) = tokio::join!(
        first_agent.run_observed_scoped(
            &first_scope,
            &mut first_history,
            &first_context,
            &NoopSink,
            &first_observer,
        ),
        second_agent.run_observed_scoped(
            &second_scope,
            &mut second_history,
            &second_context,
            &NoopSink,
            &second_observer,
        ),
    );
    first.unwrap();
    second.unwrap();

    assert_eq!(
        state.scoped_calls.load(Ordering::SeqCst),
        2,
        "scoped override未被调用：共享Tool的并发run仍走legacy execute"
    );
    assert_eq!(state.legacy_calls.load(Ordering::SeqCst), 0);
    let records = state.records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.contains(&ScopedContextRecord {
        identity: first_scope.identity(),
        observer_address: observer_address(&first_observer),
        cwd: PathBuf::from("first-cwd"),
        read_root: Some(PathBuf::from("first-read-root")),
    }));
    assert!(records.contains(&ScopedContextRecord {
        identity: second_scope.identity(),
        observer_address: observer_address(&second_observer),
        cwd: PathBuf::from("second-cwd"),
        read_root: Some(PathBuf::from("second-read-root")),
    }));
}

#[tokio::test]
async fn legacy_and_equivalent_root_scoped_runs_match_requests_history_and_outcomes() {
    let legacy_provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("c1", "read")]),
        response("done"),
    ]));
    let scoped_provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("c1", "read")]),
        response("done"),
    ]));
    let legacy_agent = Agent::new(
        legacy_provider.clone(),
        registry_with_immediate("read"),
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let scoped_agent = Agent::new(
        scoped_provider.clone(),
        registry_with_immediate("read"),
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["read"], &[PermissionLevel::ReadOnly], 4, None);
    let mut legacy_history = vec![Message::User("go".to_string())];
    let mut scoped_history = legacy_history.clone();

    let legacy_result = legacy_agent
        .run(&mut legacy_history, &ctx(), &NoopSink)
        .await
        .unwrap();
    let scoped_result = scoped_agent
        .run_scoped(&root, &mut scoped_history, &ctx(), &NoopSink)
        .await
        .unwrap();

    assert_eq!(legacy_result, scoped_result);
    assert_eq!(legacy_history, scoped_history);
    assert_eq!(
        *legacy_provider.recorded_requests(),
        *scoped_provider.recorded_requests()
    );
}

#[tokio::test]
async fn scoped_run_uses_min_iteration_limit_and_scoped_identity_callbacks() {
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("c1", "read")]),
        response_with_usage("forced"),
    ]));
    let agent = Agent::new(
        provider.clone(),
        registry_with_immediate("read"),
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["read"], &[PermissionLevel::ReadOnly], 1, None);
    let observer = IdentityObserver::default();
    let mut history = vec![Message::User("go".to_string())];

    let result = agent
        .run_observed_scoped(&root, &mut history, &ctx(), &NoopSink, &observer)
        .await
        .unwrap();

    assert_eq!(result, "forced");
    let requests = provider.recorded_requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].tools.is_empty());
    let identities = observer.identities.lock().unwrap();
    assert!(!identities.is_empty());
    assert!(identities
        .iter()
        .all(|identity| *identity == root.identity()));
    assert_eq!(observer.legacy_status_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_scoped_runs_are_attributable_by_distinct_run_id() {
    let first_agent = Agent::new(
        Arc::new(MockProvider::new(vec![response("one")])),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    let second_agent = Agent::new(
        Arc::new(MockProvider::new(vec![response("two")])),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    let first_scope = scope(&[], &[], 2, None);
    let second_scope = scope(&[], &[], 2, None);
    let observer = IdentityObserver::default();
    let mut first_history = vec![Message::User("one".to_string())];
    let mut second_history = vec![Message::User("two".to_string())];
    let first_ctx = ctx();
    let second_ctx = ctx();
    let sink = NoopSink;

    let (first, second) = tokio::join!(
        first_agent.run_observed_scoped(
            &first_scope,
            &mut first_history,
            &first_ctx,
            &sink,
            &observer
        ),
        second_agent.run_observed_scoped(
            &second_scope,
            &mut second_history,
            &second_ctx,
            &sink,
            &observer
        )
    );

    assert_eq!(first.unwrap(), "one");
    assert_eq!(second.unwrap(), "two");
    let ids = observer
        .identities
        .lock()
        .unwrap()
        .iter()
        .map(|identity| identity.run_id())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from([
            first_scope.identity().run_id(),
            second_scope.identity().run_id()
        ])
    );
}

#[tokio::test(start_paused = true)]
async fn provider_wait_cancellation_returns_scoped_error_without_assistant() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let agent = Agent::new(
        Arc::new(PendingProvider {
            entered: Mutex::new(Some(entered_tx)),
        }),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    let root = scope(&[], &[], 2, None);
    let mut history = vec![Message::User("wait".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before provider entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("cancelled provider future was not dropped"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert!(
        history.is_empty(),
        "Provider 返回 Assistant 前取消时，未提交的当前 User turn 必须从模型 history 回滚"
    );
}

#[tokio::test(start_paused = true)]
async fn provider_wait_deadline_is_distinct_from_provider_timeout() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let agent = Agent::new(
        Arc::new(PendingProvider {
            entered: Mutex::new(Some(entered_tx)),
        }),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let root = scope(&[], &[], 2, Some(deadline));
    let mut history = vec![Message::User("wait".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before provider entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("deadline did not terminate provider future"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::DeadlineExceeded));
    assert!(
        history.is_empty(),
        "Provider 返回 Assistant 前超时时，未提交的当前 User turn 必须回滚"
    );
}

#[tokio::test(start_paused = true)]
async fn context_prepare_wait_is_cancelled_without_provider_call() {
    let provider = Arc::new(MockProvider::new(vec![response("unreachable")]));
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let mut agent = Agent::new(
        provider.clone(),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    agent.set_strategy(Box::new(PendingStrategy {
        entered: Mutex::new(Some(entered_tx)),
    }));
    let root = scope(&[], &[], 2, None);
    let mut history = vec![Message::User("wait".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before context entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("context future was not dropped"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert!(provider.recorded_requests().is_empty());
    assert!(
        history.is_empty(),
        "context preparation 完成前取消时，未提交的当前 User turn 必须回滚"
    );
}

#[tokio::test(start_paused = true)]
async fn forced_final_provider_wait_can_be_cancelled() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let agent = Agent::new(
        Arc::new(PendingProvider {
            entered: Mutex::new(Some(entered_tx)),
        }),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        0,
    );
    let root = scope(&[], &[], 0, None);
    let mut history = vec![Message::User("force".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("forced final ended before provider entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("forced-final future was not dropped"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert!(!matches!(
        result,
        Err(ScopedAgentError::Agent(
            crate::error::AgentError::MaxIterations { .. }
        ))
    ));
    assert!(
        history.is_empty(),
        "forced-final 返回 Assistant 前取消时，未提交的当前 User turn 必须回滚"
    );
}

#[tokio::test(start_paused = true)]
async fn permission_wait_cancellation_closes_current_occurrence_without_execute() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let executions = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(ExecuteTool {
            executions: executions.clone(),
        }))
        .unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("permission-1", "execute")]),
        response("unreachable"),
    ]));
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(PendingDecider {
            entered: Mutex::new(Some(entered_tx)),
        }),
        "model".to_string(),
        4,
    );
    let root = scope(&["execute"], &[PermissionLevel::Execute], 4, None);
    let mut history = vec![Message::User("execute".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before permission entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("permission future was not dropped"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    assert_eq!(provider.recorded_requests().len(), 1);
    assert!(matches!(
        history.as_slice(),
        [
            Message::User(prompt),
            Message::Assistant { .. },
            Message::ToolResult { .. }
        ] if prompt == "execute"
    ));
    assert_eq!(
        tool_results(&history),
        vec![(
            "permission-1".to_string(),
            "tool call interrupted before completion".to_string(),
            true
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn serial_execute_cancellation_closes_current_and_later_occurrences() {
    let (first_tool, mut first_watch) = ControlledTool::pair("first", ToolConcurrency::Exclusive);
    let (second_tool, mut second_watch) =
        ControlledTool::pair("second", ToolConcurrency::Exclusive);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(first_tool)).unwrap();
    registry.register(Box::new(second_tool)).unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call("c1", "first"), call("c2", "second")]),
        response("unreachable"),
    ]));
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["first", "second"], &[PermissionLevel::ReadOnly], 4, None);
    let mut history = vec![Message::User("go".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before serial tool entered: {result:?}"),
        entered = &mut first_watch.entered => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("serial execute future was not dropped"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(provider.recorded_requests().len(), 1);
    assert_eq!(
        tool_results(&history),
        vec![
            (
                "c1".to_string(),
                "tool call interrupted before completion".to_string(),
                true
            ),
            (
                "c2".to_string(),
                "tool call interrupted before completion".to_string(),
                true
            )
        ]
    );
    assert!(matches!(
        second_watch.entered.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
}

#[tokio::test(start_paused = true)]
async fn serial_execute_deadline_uses_distinct_synthetic_result() {
    let (tool, mut watch) = ControlledTool::pair("slow", ToolConcurrency::Exclusive);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(tool)).unwrap();
    let provider = Arc::new(MockProvider::new(vec![tool_response(vec![call(
        "deadline-1",
        "slow",
    )])]));
    let agent = Agent::new(
        provider,
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let root = scope(&["slow"], &[PermissionLevel::ReadOnly], 4, Some(deadline));
    let mut history = vec![Message::User("go".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run = Box::pin(agent.run_scoped(&root, &mut history, &tool_ctx, &sink));

    tokio::select! {
        result = &mut run => panic!("run ended before tool entered: {result:?}"),
        entered = &mut watch.entered => entered.unwrap(),
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("deadline did not drop serial execute"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::DeadlineExceeded));
    assert_eq!(
        tool_results(&history),
        vec![(
            "deadline-1".to_string(),
            "tool call deadline exceeded before completion".to_string(),
            true
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn parallel_prefix_is_preserved_and_remaining_occurrence_is_synthetic() {
    let (first_tool, mut first_watch) = ControlledTool::pair("p1", ToolConcurrency::ParallelSafe);
    let (second_tool, mut second_watch) = ControlledTool::pair("p2", ToolConcurrency::ParallelSafe);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(first_tool)).unwrap();
    registry.register(Box::new(second_tool)).unwrap();
    let provider = Arc::new(MockProvider::new(vec![tool_response(vec![
        call("p1-id", "p1"),
        call("p2-id", "p2"),
    ])]));
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["p1", "p2"], &[PermissionLevel::ReadOnly], 4, None);
    let (finished_tx, mut finished_rx) = oneshot::channel();
    let observer = FinishObserver::new(finished_tx);
    let mut history = vec![Message::User("go".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run =
        Box::pin(agent.run_observed_scoped(&root, &mut history, &tool_ctx, &sink, &observer));

    tokio::select! {
        result = &mut run => panic!("run ended before p1 entered: {result:?}"),
        entered = &mut first_watch.entered => entered.unwrap(),
    }
    tokio::select! {
        result = &mut run => panic!("run ended before p2 entered: {result:?}"),
        entered = &mut second_watch.entered => entered.unwrap(),
    }
    let _ = first_watch.release.take().unwrap().send(());
    tokio::select! {
        result = &mut run => panic!("run ended before prefix published: {result:?}"),
        finished = &mut finished_rx => finished.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("parallel remainder was not cancelled"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(provider.recorded_requests().len(), 1);
    assert_eq!(
        tool_results(&history),
        vec![
            ("p1-id".to_string(), "ok:p1".to_string(), false),
            (
                "p2-id".to_string(),
                "tool call interrupted before completion".to_string(),
                true
            )
        ]
    );
    assert_eq!(observer.finished.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn parallel_ready_but_unpublished_duplicate_occurrences_are_discarded() {
    let (first_tool, mut first_watch) = ControlledTool::pair("p1", ToolConcurrency::ParallelSafe);
    let (second_tool, mut second_watch) = ControlledTool::pair("p2", ToolConcurrency::ParallelSafe);
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(first_tool)).unwrap();
    registry.register(Box::new(second_tool)).unwrap();
    let provider = Arc::new(MockProvider::new(vec![tool_response(vec![
        call("dup", "p1"),
        call("dup", "p2"),
    ])]));
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["p1", "p2"], &[PermissionLevel::ReadOnly], 4, None);
    let (finished_tx, _finished_rx) = oneshot::channel();
    let observer = FinishObserver::new(finished_tx);
    let mut history = vec![Message::User("go".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run =
        Box::pin(agent.run_observed_scoped(&root, &mut history, &tool_ctx, &sink, &observer));

    tokio::select! {
        result = &mut run => panic!("run ended before p1 entered: {result:?}"),
        entered = &mut first_watch.entered => entered.unwrap(),
    }
    tokio::select! {
        result = &mut run => panic!("run ended before p2 entered: {result:?}"),
        entered = &mut second_watch.entered => entered.unwrap(),
    }
    let _ = second_watch.release.take().unwrap().send(());
    tokio::select! {
        result = &mut run => panic!("run ended before p2 physical completion: {result:?}"),
        completed = &mut second_watch.completed => completed.unwrap(),
    }
    tokio::task::yield_now().await;
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("ready suffix was not discarded"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(provider.recorded_requests().len(), 1);
    assert_eq!(
        tool_results(&history),
        vec![
            (
                "dup".to_string(),
                "tool call interrupted before completion".to_string(),
                true
            ),
            (
                "dup".to_string(),
                "tool call interrupted before completion".to_string(),
                true
            )
        ]
    );
    assert!(observer.finished.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_spawn_blocking_work_finishes_naturally_without_publishing_late_outcome() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let (completed_tx, completed_rx) = oneshot::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let mut registry = ToolRegistry::new();
    registry
        .register(Box::new(BlockingControlledTool {
            entered: Mutex::new(Some(entered_tx)),
            completed: Mutex::new(Some(completed_tx)),
            release: release.clone(),
            limiter: BlockingToolLimiter::new(4),
        }))
        .unwrap();
    let provider = Arc::new(MockProvider::new(vec![tool_response(vec![call(
        "blocking-1",
        "blocking",
    )])]));
    let agent = Agent::new(
        provider.clone(),
        registry,
        Box::new(AllowAll),
        "model".to_string(),
        4,
    );
    let root = scope(&["blocking"], &[PermissionLevel::ReadOnly], 4, None);
    let (finished_tx, _finished_rx) = oneshot::channel();
    let observer = FinishObserver::new(finished_tx);
    let mut history = vec![Message::User("go".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run =
        Box::pin(agent.run_observed_scoped(&root, &mut history, &tool_ctx, &sink, &observer));

    let watchdog_release = release.clone();
    let watchdog_fired = Arc::new(AtomicBool::new(false));
    let watchdog_fired_thread = watchdog_fired.clone();
    let (watchdog_cancel_tx, watchdog_cancel_rx) = std::sync::mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if watchdog_cancel_rx
            .recv_timeout(Duration::from_secs(5))
            .is_err()
        {
            watchdog_fired_thread.store(true, Ordering::SeqCst);
            let (released, wake) = &*watchdog_release;
            *released.lock().unwrap() = true;
            wake.notify_all();
        }
    });

    tokio::select! {
        result = &mut run => panic!("run ended before blocking closure entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = (&mut run).await;
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(provider.recorded_requests().len(), 1);
    assert_eq!(
        tool_results(&history),
        vec![(
            "blocking-1".to_string(),
            "tool call interrupted before completion".to_string(),
            true
        )]
    );
    assert!(observer.finished.lock().unwrap().is_empty());

    {
        let (released, wake) = &*release;
        *released.lock().unwrap() = true;
        wake.notify_all();
    }
    completed_rx.await.unwrap();
    tokio::task::yield_now().await;
    assert!(observer.finished.lock().unwrap().is_empty());
    assert_eq!(tool_results(&history).len(), 1);

    let _ = watchdog_cancel_tx.send(());
    watchdog.join().unwrap();
    assert!(!watchdog_fired.load(Ordering::SeqCst));
}

#[derive(Clone, Debug, PartialEq)]
enum ScopedLifecycleEvent {
    Status(RunIdentity, AgentStatus),
    Usage(RunIdentity, Usage),
}

#[derive(Default)]
struct ScopedLifecycleObserver {
    events: Mutex<Vec<ScopedLifecycleEvent>>,
}

impl ScopedLifecycleObserver {
    fn events(&self) -> Vec<ScopedLifecycleEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentObserver for ScopedLifecycleObserver {
    fn on_scoped_status(&self, identity: &RunIdentity, status: AgentStatus) {
        self.events
            .lock()
            .unwrap()
            .push(ScopedLifecycleEvent::Status(*identity, status));
    }

    fn on_scoped_usage(&self, identity: &RunIdentity, usage: &Usage) {
        self.events
            .lock()
            .unwrap()
            .push(ScopedLifecycleEvent::Usage(*identity, usage.clone()));
    }
}

#[tokio::test]
async fn forced_final_success_emits_calling_usage_idle_in_scope_order() {
    let usage = Usage {
        input_tokens: 2,
        output_tokens: 3,
    };
    let agent = Agent::new(
        Arc::new(MockProvider::new(vec![response_with_usage("forced")])),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        0,
    );
    let root = scope(&[], &[], 0, None);
    let identity = root.identity();
    let observer = ScopedLifecycleObserver::default();
    let mut history = vec![Message::User("force".to_string())];

    let result = agent
        .run_observed_scoped(&root, &mut history, &ctx(), &NoopSink, &observer)
        .await;

    assert_eq!(result, Ok("forced".to_string()));
    assert_eq!(
        observer.events(),
        vec![
            ScopedLifecycleEvent::Status(identity, AgentStatus::CallingModel),
            ScopedLifecycleEvent::Usage(identity, usage),
            ScopedLifecycleEvent::Status(identity, AgentStatus::Idle),
        ]
    );
}

#[tokio::test]
async fn forced_final_provider_error_emits_calling_without_usage_or_idle() {
    let agent = Agent::new(
        Arc::new(MockProvider::new(Vec::new())),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        0,
    );
    let root = scope(&[], &[], 0, None);
    let identity = root.identity();
    let observer = ScopedLifecycleObserver::default();
    let mut history = vec![Message::User("force".to_string())];

    let result = agent
        .run_observed_scoped(&root, &mut history, &ctx(), &NoopSink, &observer)
        .await;

    assert!(matches!(
        result,
        Err(ScopedAgentError::Agent(crate::error::AgentError::Provider(
            ProviderError::Transport(_)
        )))
    ));
    assert_eq!(
        observer.events(),
        vec![ScopedLifecycleEvent::Status(
            identity,
            AgentStatus::CallingModel,
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn forced_final_termination_emits_calling_without_usage_or_idle() {
    let (entered_tx, mut entered_rx) = oneshot::channel();
    let agent = Agent::new(
        Arc::new(PendingProvider {
            entered: Mutex::new(Some(entered_tx)),
        }),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        0,
    );
    let root = scope(&[], &[], 0, None);
    let identity = root.identity();
    let observer = ScopedLifecycleObserver::default();
    let mut history = vec![Message::User("force".to_string())];
    let tool_ctx = ctx();
    let sink = NoopSink;
    let mut run =
        Box::pin(agent.run_observed_scoped(&root, &mut history, &tool_ctx, &sink, &observer));

    tokio::select! {
        result = &mut run => panic!("forced final ended before Provider entered: {result:?}"),
        entered = &mut entered_rx => entered.unwrap(),
    }
    root.cancel();
    let result = tokio::select! {
        biased;
        result = &mut run => result,
        _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("forced final did not terminate"),
    };
    drop(run);

    assert_eq!(result, Err(ScopedAgentError::Cancelled));
    assert_eq!(
        observer.events(),
        vec![ScopedLifecycleEvent::Status(
            identity,
            AgentStatus::CallingModel,
        )]
    );
}

#[tokio::test]
async fn normal_empty_response_emits_usage_but_never_idle() {
    let usage = Usage {
        input_tokens: 2,
        output_tokens: 3,
    };
    let agent = Agent::new(
        Arc::new(MockProvider::new(vec![response_with_usage("")])),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        2,
    );
    let root = scope(&[], &[], 2, None);
    let identity = root.identity();
    let observer = ScopedLifecycleObserver::default();
    let mut history = vec![Message::User("empty".to_string())];

    let result = agent
        .run_observed_scoped(&root, &mut history, &ctx(), &NoopSink, &observer)
        .await;

    assert_eq!(result, Ok(String::new()));
    assert_eq!(
        observer.events(),
        vec![
            ScopedLifecycleEvent::Status(identity, AgentStatus::CallingModel),
            ScopedLifecycleEvent::Usage(identity, usage),
        ]
    );
}

#[tokio::test]
async fn forced_final_empty_response_emits_calling_and_usage_but_never_idle() {
    let usage = Usage {
        input_tokens: 2,
        output_tokens: 3,
    };
    let agent = Agent::new(
        Arc::new(MockProvider::new(vec![response_with_usage("")])),
        ToolRegistry::new(),
        Box::new(AllowAll),
        "model".to_string(),
        0,
    );
    let root = scope(&[], &[], 0, None);
    let identity = root.identity();
    let observer = ScopedLifecycleObserver::default();
    let mut history = vec![Message::User("empty".to_string())];

    let result = agent
        .run_observed_scoped(&root, &mut history, &ctx(), &NoopSink, &observer)
        .await;

    assert_eq!(
        result,
        Err(ScopedAgentError::Agent(
            crate::error::AgentError::MaxIterations { limit: 0 }
        ))
    );
    assert_eq!(
        observer.events(),
        vec![
            ScopedLifecycleEvent::Status(identity, AgentStatus::CallingModel),
            ScopedLifecycleEvent::Usage(identity, usage),
        ]
    );
}

#[test]
fn legacy_observer_implementation_remains_source_compatible() {
    struct LegacyOnlyObserver;
    impl AgentObserver for LegacyOnlyObserver {
        fn on_status(&self, _status: AgentStatus) {}
    }

    let _observer: Box<dyn AgentObserver> = Box::new(LegacyOnlyObserver);
    assert_eq!(PermissionMode::Normal, PermissionMode::Normal);
}
