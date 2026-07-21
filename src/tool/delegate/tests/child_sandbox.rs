use super::super::{DelegateTaskTool, DELEGATE_TASK_NAME, SUBAGENT_SYSTEM_PROMPT};
use crate::agent::{
    message::Message, AgentExecutionScope, AgentObserver, AgentRuntime, ExecutionBudget,
    ExecutionCapabilities, RunIdentity,
};
use crate::error::ProviderError;
use crate::provider::{
    DeltaSink, Depth, FinishReason, ModelRequest, ModelResponse, Provider, ThinkingConfig, ToolCall,
};
use crate::tool::fs::{GlobTool, GrepTool, ListDirTool, ReadFileTool};
use crate::tool::{
    BlockingToolLimiter, NetworkPermissionPreview, PermissionLevel, Tool, ToolContext,
    ToolExecutionContext, ToolOutcome, ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::oneshot;

const CHILD_TOOL_NAMES: [&str; 4] = ["list_dir", "read_file", "glob", "grep"];

fn child_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ListDirTool)).unwrap();
    registry.register(Box::new(ReadFileTool)).unwrap();
    registry.register(Box::new(GlobTool)).unwrap();
    registry.register(Box::new(GrepTool)).unwrap();
    registry
}

fn parent_scope() -> AgentExecutionScope {
    AgentExecutionScope::root(
        ExecutionBudget::new(8, None, 1),
        ExecutionCapabilities::try_new(
            [DELEGATE_TASK_NAME].into_iter().chain(CHILD_TOOL_NAMES),
            [PermissionLevel::ReadOnly],
        )
        .unwrap(),
    )
}

fn tool_context(cwd: PathBuf) -> ToolContext {
    ToolContext {
        cwd,
        max_output_bytes: 16 * 1024,
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

fn tool_response(tool_calls: Vec<ToolCall>) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls,
        finish_reason: FinishReason::ToolCalls,
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

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

struct ScriptProvider {
    name: &'static str,
    responses: Mutex<VecDeque<ModelResponse>>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl ScriptProvider {
    fn new(name: &'static str, responses: Vec<ModelResponse>) -> Self {
        Self {
            name,
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> MutexGuard<'_, Vec<ModelRequest>> {
        self.requests.lock().unwrap()
    }
}

#[async_trait]
impl Provider for ScriptProvider {
    fn name(&self) -> &str {
        self.name
    }

    async fn complete(
        &self,
        request: ModelRequest,
        _sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ProviderError::Transport("test provider script exhausted".to_string()))
    }
}

async fn invoke(
    tool: &DelegateTaskTool,
    scope: &AgentExecutionScope,
    context: &ToolContext,
    observer: &dyn AgentObserver,
    task: &str,
) -> ToolOutcome {
    tool.execute_scoped(
        json!({ "task": task }),
        &ToolExecutionContext {
            tool: context,
            scope,
            observer,
            read_root: None,
        },
    )
    .await
}

#[tokio::test]
async fn valid_child_uses_exact_isolated_history_and_normal_low_passthrough_request() {
    let temp = tempfile::tempdir().unwrap();
    let raw_task = "  inspect this workspace exactly\n";
    let provider = Arc::new(ScriptProvider::new(
        "invocation-provider",
        vec![final_response("child report")],
    ));
    let runtime = AgentRuntime::new(provider.clone(), "invocation-model".to_string());
    let tool = DelegateTaskTool::with_dependencies(
        runtime,
        child_registry(),
        Arc::new(|path| std::fs::canonicalize(path)),
        BlockingToolLimiter::new(4),
    );

    let outcome = invoke(
        &tool,
        &parent_scope(),
        &tool_context(temp.path().to_path_buf()),
        &crate::agent::NoopObserver,
        raw_task,
    )
    .await;
    let requests = provider.requests();

    assert_eq!(
        requests.len(),
        1,
        "有效delegate必须创建一个临时child并发起首个Provider请求；outcome={outcome:?}"
    );
    let request = &requests[0];
    assert_eq!(request.model, "invocation-model");
    assert_eq!(
        request.messages,
        vec![
            Message::System(SUBAGENT_SYSTEM_PROMPT.to_string()),
            Message::User(raw_task.to_string()),
        ],
        "child不得继承parent history/System/thinking/permission mode/plan/session，且task必须逐字节保留"
    );
    assert_eq!(
        request.thinking,
        Some(ThinkingConfig { depth: Depth::Low }),
        "child thinking必须固定为Low"
    );
    assert_eq!(
        request
            .tools
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<Vec<_>>(),
        CHILD_TOOL_NAMES,
        "Passthrough + Normal child首请求必须只暴露四个只读工具"
    );
    assert!(
        !outcome.is_error,
        "有效child返回最终文本后delegate必须成功：{outcome:?}"
    );
}

struct FirstRoundBarrierProvider {
    requests: Mutex<Vec<ModelRequest>>,
    calls: AtomicUsize,
    entered: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl Provider for FirstRoundBarrierProvider {
    fn name(&self) -> &str {
        "old-provider"
    }

    async fn complete(
        &self,
        request: ModelRequest,
        _sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        self.requests.lock().unwrap().push(request);
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        if index == 0 {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            let release = self
                .release
                .lock()
                .unwrap()
                .take()
                .expect("first call must own the release receiver");
            release
                .await
                .map_err(|_| ProviderError::Transport("test release dropped".to_string()))?;
            Ok(tool_response(vec![call(
                "old-read",
                "read_file",
                json!({ "path": "inside.txt" }),
            )]))
        } else {
            Ok(final_response("old child finished"))
        }
    }
}

#[tokio::test]
async fn running_child_keeps_invocation_tuple_and_next_delegate_uses_replaced_pair() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("inside.txt"), "inside").unwrap();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let old_provider = Arc::new(FirstRoundBarrierProvider {
        requests: Mutex::new(Vec::new()),
        calls: AtomicUsize::new(0),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(Some(release_rx)),
    });
    let new_provider = Arc::new(ScriptProvider::new(
        "new-provider",
        vec![final_response("new child finished")],
    ));
    let runtime = AgentRuntime::new(old_provider.clone(), "old-model".to_string());
    let tool = Arc::new(DelegateTaskTool::with_dependencies(
        runtime.clone(),
        child_registry(),
        Arc::new(|path| std::fs::canonicalize(path)),
        BlockingToolLimiter::new(4),
    ));
    let cwd = temp.path().to_path_buf();
    let first_tool = tool.clone();
    let first = tokio::spawn(async move {
        invoke(
            first_tool.as_ref(),
            &parent_scope(),
            &tool_context(cwd),
            &crate::agent::NoopObserver,
            "first invocation",
        )
        .await
    });

    if tokio::time::timeout(Duration::from_secs(1), entered_rx)
        .await
        .is_err()
    {
        first.abort();
        let _ = first.await;
        panic!("有效delegate未进入child首轮Provider，无法验证invocation snapshot");
    }

    runtime.replace_provider_model(new_provider.clone(), "new-model".to_string());
    release_tx.send(()).unwrap();
    let first_outcome = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("first delegate did not finish after deterministic release")
        .expect("first delegate task panicked");
    assert!(
        !first_outcome.is_error,
        "first delegate failed unexpectedly: {first_outcome:?}"
    );

    let second_outcome = tokio::time::timeout(
        Duration::from_secs(2),
        invoke(
            tool.as_ref(),
            &parent_scope(),
            &tool_context(temp.path().to_path_buf()),
            &crate::agent::NoopObserver,
            "second invocation",
        ),
    )
    .await
    .expect("second delegate timed out");
    assert!(
        !second_outcome.is_error,
        "second delegate failed unexpectedly: {second_outcome:?}"
    );

    let old_requests = old_provider.requests.lock().unwrap();
    assert_eq!(
        old_requests.len(),
        2,
        "同一child两轮必须始终使用invocation时冻结的old Provider"
    );
    assert!(old_requests
        .iter()
        .all(|request| request.model == "old-model"));
    let new_requests = new_provider.requests();
    assert_eq!(
        new_requests.len(),
        1,
        "runtime pair replace后仅下一次delegate应使用new Provider"
    );
    assert_eq!(new_requests[0].model, "new-model");
}

#[derive(Default)]
struct ForbiddenCounters {
    preview: AtomicUsize,
    execute: AtomicUsize,
    ui: AtomicUsize,
}

struct ForbiddenProbeTool {
    name: &'static str,
    level: PermissionLevel,
    plan_only: bool,
    counts_as_ui: bool,
    counters: Arc<ForbiddenCounters>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CapabilityObservation {
    tool_names: BTreeSet<String>,
    permission_levels: BTreeSet<PermissionLevel>,
}

struct ScopedReadFileProbe {
    observed: Arc<Mutex<Option<CapabilityObservation>>>,
}

#[async_trait]
impl Tool for ScopedReadFileProbe {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        ReadFileTool.description()
    }

    fn schema(&self) -> Value {
        ReadFileTool.schema()
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn concurrency(&self) -> crate::tool::ToolConcurrency {
        ReadFileTool.concurrency()
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        ReadFileTool.execute(args, ctx).await
    }

    async fn execute_scoped(&self, args: Value, ctx: &ToolExecutionContext<'_>) -> ToolOutcome {
        *self.observed.lock().unwrap() = Some(CapabilityObservation {
            tool_names: ctx.scope.capabilities().tool_names().clone(),
            permission_levels: ctx.scope.capabilities().permission_levels().clone(),
        });
        ReadFileTool.execute_scoped(args, ctx).await
    }
}

#[async_trait]
impl Tool for ForbiddenProbeTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "forbidden child sandbox probe"
    }

    fn schema(&self) -> Value {
        json!({ "type": "object" })
    }

    fn permission_level(&self) -> PermissionLevel {
        self.level.clone()
    }

    fn plan_only(&self) -> bool {
        self.plan_only
    }

    fn network_permission_preview(&self, args: &Value) -> NetworkPermissionPreview {
        self.counters.preview.fetch_add(1, Ordering::SeqCst);
        NetworkPermissionPreview {
            authorizable: true,
            full_args: args.clone(),
            canonical_initial_target: Some("https://example.invalid/".to_string()),
            scope: None,
            denial_reason: None,
        }
    }

    async fn execute(&self, _args: Value, _ctx: &ToolContext) -> ToolOutcome {
        self.counters.execute.fetch_add(1, Ordering::SeqCst);
        if self.counts_as_ui {
            self.counters.ui.fetch_add(1, Ordering::SeqCst);
        }
        ToolOutcome {
            content: "forbidden probe executed".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

#[derive(Default)]
struct StartedObserver {
    names: Mutex<Vec<String>>,
}

impl AgentObserver for StartedObserver {
    fn on_scoped_tool_call_started(
        &self,
        _identity: &RunIdentity,
        _id: &str,
        name: &str,
        _args: &Value,
        _readonly: bool,
    ) {
        self.names.lock().unwrap().push(name.to_string());
    }
}

#[tokio::test]
async fn child_schema_capability_and_lookup_exclude_all_forbidden_tools() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("inside.txt"), "inside").unwrap();
    let observed_capabilities = Arc::new(Mutex::new(None));
    let mut root_registry = ToolRegistry::new();
    root_registry.register(Box::new(ListDirTool)).unwrap();
    root_registry
        .register(Box::new(ScopedReadFileProbe {
            observed: observed_capabilities.clone(),
        }))
        .unwrap();
    root_registry.register(Box::new(GlobTool)).unwrap();
    root_registry.register(Box::new(GrepTool)).unwrap();
    let forbidden = [
        ("web_fetch", PermissionLevel::Network, false, false),
        ("write_file", PermissionLevel::Edit, false, false),
        ("edit_file", PermissionLevel::Edit, false, false),
        ("run_shell", PermissionLevel::Execute, false, false),
        ("ask_user", PermissionLevel::ReadOnly, false, true),
        ("submit_plan", PermissionLevel::ReadOnly, true, true),
        ("update_plan", PermissionLevel::ReadOnly, true, true),
        (DELEGATE_TASK_NAME, PermissionLevel::ReadOnly, false, false),
    ];
    let mut counters = Vec::new();
    for (name, level, plan_only, counts_as_ui) in forbidden {
        let state = Arc::new(ForbiddenCounters::default());
        root_registry
            .register(Box::new(ForbiddenProbeTool {
                name,
                level,
                plan_only,
                counts_as_ui,
                counters: state.clone(),
            }))
            .unwrap();
        counters.push((name, state));
    }
    let restricted = root_registry.restricted_to(CHILD_TOOL_NAMES).unwrap();
    let forbidden_calls = [
        ("forbidden-web", "web_fetch"),
        ("forbidden-write", "write_file"),
        ("forbidden-edit", "edit_file"),
        ("forbidden-shell", "run_shell"),
        ("forbidden-ask", "ask_user"),
        ("forbidden-plan", "submit_plan"),
        ("forbidden-update", "update_plan"),
        ("forbidden-delegate", DELEGATE_TASK_NAME),
    ];
    let mut calls = vec![call(
        "allowed-read",
        "read_file",
        json!({ "path": "inside.txt" }),
    )];
    calls.extend(
        forbidden_calls
            .iter()
            .map(|(id, name)| call(id, name, json!({}))),
    );
    let provider = Arc::new(ScriptProvider::new(
        "sandbox-provider",
        vec![tool_response(calls), final_response("sandbox complete")],
    ));
    let tool = DelegateTaskTool::with_dependencies(
        AgentRuntime::new(provider.clone(), "sandbox-model".to_string()),
        restricted,
        Arc::new(|path| std::fs::canonicalize(path)),
        BlockingToolLimiter::new(4),
    );
    let observer = StartedObserver::default();

    let outcome = tokio::time::timeout(
        Duration::from_secs(2),
        invoke(
            &tool,
            &parent_scope(),
            &tool_context(temp.path().to_path_buf()),
            &observer,
            "probe sandbox",
        ),
    )
    .await
    .expect("sandbox delegate timed out");
    let requests = provider.requests();

    assert!(
        !outcome.is_error,
        "sandbox delegate failed unexpectedly: {outcome:?}"
    );
    assert_eq!(
        requests.len(),
        2,
        "child must continue after fail-closed calls"
    );
    assert_eq!(
        requests[0]
            .tools
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<Vec<_>>(),
        CHILD_TOOL_NAMES,
        "child schema/capability必须恰含四个只读工具且保持root顺序"
    );
    assert_eq!(
        requests[0].thinking,
        Some(ThinkingConfig { depth: Depth::Low })
    );
    assert_eq!(
        *observed_capabilities.lock().unwrap(),
        Some(CapabilityObservation {
            tool_names: CHILD_TOOL_NAMES.into_iter().map(str::to_string).collect(),
            permission_levels: [PermissionLevel::ReadOnly].into_iter().collect(),
        }),
        "child execution capability必须恰含四个工具名与ReadOnly"
    );

    for (id, name) in forbidden_calls {
        assert!(
            requests[1].messages.iter().any(|message| {
                matches!(
                    message,
                    Message::ToolResult {
                        call_id,
                        is_error: true,
                        ..
                    } if call_id == id
                )
            }),
            "child hard-sent forbidden tool `{name}` did not fail closed"
        );
    }
    assert_eq!(
        observer.names.lock().unwrap().as_slice(),
        &["read_file"],
        "forbidden child calls不得进入tool-started/UI路径"
    );
    for (name, state) in counters {
        assert_eq!(
            state.preview.load(Ordering::SeqCst),
            0,
            "{name} reached network preview"
        );
        assert_eq!(
            state.execute.load(Ordering::SeqCst),
            0,
            "{name} reached target execute"
        );
        assert_eq!(
            state.ui.load(Ordering::SeqCst),
            0,
            "{name} reached ask/plan/update UI collaborator"
        );
    }
}
