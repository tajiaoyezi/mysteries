use super::super::{DelegateTaskTool, DELEGATE_TASK_NAME};
use crate::agent::message::Message;
use crate::agent::{
    Agent, AgentExecutionScope, AgentObserver, AgentRuntime, AgentStatus, ExecutionBudget,
    ExecutionCapabilities, RunIdentity,
};
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use crate::tool::fs::{GlobTool, GrepTool, ListDirTool};
use crate::tool::{
    BlockingToolLimiter, PermissionLevel, Tool, ToolConcurrency, ToolContext, ToolExecutionContext,
    ToolOutcome, ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const EXPECTED_DESCRIPTION: &str =
    "Delegate an independent read-only workspace research task and return an untrusted report.";

fn final_response(text: &str) -> ModelResponse {
    ModelResponse {
        text: text.to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: None,
        thinking: Vec::new(),
    }
}

fn tool_response(name: &str, arguments: Value) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: "delegate-call".to_string(),
            name: name.to_string(),
            arguments,
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
        thinking: Vec::new(),
    }
}

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

struct PanicDecider;

#[async_trait]
impl PermissionDecider for PanicDecider {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        panic!("ReadOnly delegate_task must not ask for permission")
    }
}

struct CountingReadFileTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Count unexpected child read_file execution."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        ToolOutcome {
            content: "unexpected child read".to_string(),
            is_error: false,
            truncated: false,
            exit: None,
        }
    }
}

struct DelegateFixture {
    tool: DelegateTaskTool,
    child_provider: Arc<MockProvider>,
    child_execute_calls: Arc<AtomicUsize>,
    canonicalize_calls: Arc<AtomicUsize>,
}

fn delegate_fixture(child_script: Vec<ModelResponse>) -> DelegateFixture {
    let child_provider = Arc::new(MockProvider::new(child_script));
    let runtime = AgentRuntime::new(child_provider.clone(), "child-model".to_string());
    let child_execute_calls = Arc::new(AtomicUsize::new(0));
    let mut child_registry = ToolRegistry::new();
    child_registry.register(Box::new(ListDirTool)).unwrap();
    child_registry
        .register(Box::new(CountingReadFileTool {
            calls: child_execute_calls.clone(),
        }))
        .unwrap();
    child_registry.register(Box::new(GlobTool)).unwrap();
    child_registry.register(Box::new(GrepTool)).unwrap();

    let canonicalize_calls = Arc::new(AtomicUsize::new(0));
    let calls = canonicalize_calls.clone();
    let tool = DelegateTaskTool::with_dependencies(
        runtime,
        child_registry,
        Arc::new(move |path: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            std::fs::canonicalize(path)
        }),
        BlockingToolLimiter::new(4),
    );

    DelegateFixture {
        tool,
        child_provider,
        child_execute_calls,
        canonicalize_calls,
    }
}

fn root_registry_with_delegate(delegate: DelegateTaskTool) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ListDirTool)).unwrap();
    registry
        .register(Box::new(crate::tool::fs::ReadFileTool))
        .unwrap();
    registry.register(Box::new(GlobTool)).unwrap();
    registry.register(Box::new(GrepTool)).unwrap();
    registry.register(Box::new(delegate)).unwrap();
    registry
}

#[derive(Clone, Debug)]
enum ObservedEvent {
    Status {
        identity: RunIdentity,
        _status: AgentStatus,
    },
    Started {
        identity: RunIdentity,
        id: String,
        name: String,
        args: Value,
    },
    Finished {
        identity: RunIdentity,
        id: String,
        outcome: ToolOutcome,
    },
}

impl ObservedEvent {
    fn identity(&self) -> RunIdentity {
        match self {
            Self::Status { identity, .. }
            | Self::Started { identity, .. }
            | Self::Finished { identity, .. } => *identity,
        }
    }
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
        self.events.lock().unwrap().push(ObservedEvent::Status {
            identity: *identity,
            _status: status,
        });
    }

    fn on_scoped_tool_call_started(
        &self,
        identity: &RunIdentity,
        id: &str,
        name: &str,
        args: &Value,
        _readonly: bool,
    ) {
        self.events.lock().unwrap().push(ObservedEvent::Started {
            identity: *identity,
            id: id.to_string(),
            name: name.to_string(),
            args: args.clone(),
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
            id: id.to_string(),
            outcome: outcome.clone(),
        });
    }
}

#[test]
fn delegate_task_metadata_has_exact_identity_description_and_schema() {
    let fixture = delegate_fixture(Vec::new());
    let tool = &fixture.tool;

    assert_eq!(tool.name(), DELEGATE_TASK_NAME);
    assert_eq!(tool.description(), EXPECTED_DESCRIPTION);
    assert_eq!(
        tool.schema(),
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "minLength": 1
                }
            },
            "required": ["task"],
            "additionalProperties": false
        })
    );
}

#[test]
fn delegate_task_metadata_is_readonly_parallel_non_plan_depth_one() {
    let fixture = delegate_fixture(Vec::new());
    let tool = &fixture.tool;

    assert_eq!(tool.permission_level(), PermissionLevel::ReadOnly);
    assert_eq!(tool.concurrency(), ToolConcurrency::ParallelSafe);
    assert!(!tool.plan_only());
    assert_eq!(tool.required_child_depth(), 1);
}

#[test]
fn delegate_task_network_preview_remains_unauthorizable() {
    let fixture = delegate_fixture(Vec::new());
    let args = json!({ "task": "inspect workspace" });
    let preview = fixture.tool.network_permission_preview(&args);

    assert!(!preview.authorizable);
    assert_eq!(preview.full_args, args);
    assert_eq!(preview.canonical_initial_target, None);
    assert_eq!(preview.scope, None);
    assert!(preview.denial_reason.is_some());
}

#[tokio::test]
async fn legacy_execute_requires_scoped_context_without_child_side_effects() {
    let fixture = delegate_fixture(Vec::new());
    let context = ToolContext {
        cwd: std::env::current_dir().unwrap(),
        max_output_bytes: 4096,
    };

    let first = fixture
        .tool
        .execute(json!({ "task": "inspect workspace" }), &context)
        .await;
    let second = fixture
        .tool
        .execute(json!({ "task": "inspect workspace" }), &context)
        .await;

    assert_eq!(first, second);
    assert_eq!(
        first.content,
        "delegate_task failed: scoped execution context required"
    );
    assert!(first.is_error);
    assert!(!first.truncated);
    assert_eq!(first.exit, None);
    assert_eq!(fixture.child_provider.recorded_requests().len(), 0);
    assert_eq!(fixture.child_execute_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.canonicalize_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_args_publish_one_outer_started_and_finished_without_child_side_effects() {
    let invalid_cases = [
        ("null", Value::Null),
        ("missing", json!({})),
        ("wrong-type", json!({ "task": 42 })),
        ("empty", json!({ "task": "" })),
        ("blank", json!({ "task": " \t\r\n\u{2003} " })),
        (
            "extra-field",
            json!({ "task": "valid text", "unexpected": true }),
        ),
    ];

    for (case_name, arguments) in invalid_cases {
        let fixture = delegate_fixture(Vec::new());
        let dispatch_name = fixture.tool.name().to_string();
        let child_provider = fixture.child_provider.clone();
        let child_execute_calls = fixture.child_execute_calls.clone();
        let canonicalize_calls = fixture.canonicalize_calls.clone();
        let outer_provider = Arc::new(MockProvider::new(vec![
            tool_response(&dispatch_name, arguments.clone()),
            final_response("outer complete"),
        ]));
        let agent = Agent::new(
            outer_provider.clone(),
            root_registry_with_delegate(fixture.tool),
            Box::new(PanicDecider),
            "outer-model".to_string(),
            4,
        );
        let scope = agent.product_root_scope();
        let parent_identity = scope.identity();
        let observer = RecordingObserver::default();
        let temp = tempfile::tempdir().unwrap();
        let context = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };
        let mut history = vec![Message::User("outer prompt".to_string())];

        let result = agent
            .run_observed_scoped(&scope, &mut history, &context, &NoopSink, &observer)
            .await
            .unwrap_or_else(|err| panic!("{case_name}: outer run failed: {err}"));

        assert_eq!(result, "outer complete", "{case_name}");
        let events = observer.events();
        assert!(
            events
                .iter()
                .all(|event| event.identity() == parent_identity),
            "{case_name}: invalid args emitted child observer events: {events:?}"
        );
        let started = events
            .iter()
            .filter_map(|event| match event {
                ObservedEvent::Started { id, name, args, .. } => Some((id, name, args)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(started.len(), 1, "{case_name}: {events:?}");
        assert_eq!(started[0].0, "delegate-call", "{case_name}");
        assert_eq!(started[0].1, &dispatch_name, "{case_name}");
        assert_eq!(started[0].2, &arguments, "{case_name}");

        let finished = events
            .iter()
            .filter_map(|event| match event {
                ObservedEvent::Finished { id, outcome, .. } => Some((id, outcome)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(finished.len(), 1, "{case_name}: {events:?}");
        assert_eq!(finished[0].0, "delegate-call", "{case_name}");
        assert!(finished[0].1.is_error, "{case_name}: {finished:?}");
        assert!(history.iter().any(|message| matches!(
            message,
            Message::ToolResult {
                call_id,
                is_error: true,
                ..
            } if call_id == "delegate-call"
        )));
        assert_eq!(
            outer_provider.recorded_requests().len(),
            2,
            "{case_name}: outer Provider call count drifted"
        );
        assert_eq!(
            child_provider.recorded_requests().len(),
            0,
            "{case_name}: invalid args reached child Provider"
        );
        assert_eq!(
            child_execute_calls.load(Ordering::SeqCst),
            0,
            "{case_name}: invalid args executed a child tool"
        );
        assert_eq!(
            canonicalize_calls.load(Ordering::SeqCst),
            0,
            "{case_name}: invalid args reached workspace preflight"
        );
    }
}

#[tokio::test]
async fn nonempty_task_reaches_child_provider_byte_for_byte() {
    let fixture = delegate_fixture(vec![final_response("child complete")]);
    let temp = tempfile::tempdir().unwrap();
    let context = ToolContext {
        cwd: temp.path().to_path_buf(),
        max_output_bytes: 4096,
    };
    let scope = AgentExecutionScope::root(
        ExecutionBudget::new(8, None, 1),
        ExecutionCapabilities::try_new(
            [DELEGATE_TASK_NAME, "list_dir", "read_file", "glob", "grep"],
            [PermissionLevel::ReadOnly],
        )
        .unwrap(),
    );
    let observer = RecordingObserver::default();
    let execution_context = ToolExecutionContext {
        tool: &context,
        scope: &scope,
        observer: &observer,
        read_root: None,
    };
    let task = " \t保留 task 原始字节\r\n第二行  ";

    let _outcome = fixture
        .tool
        .execute_scoped(json!({ "task": task }), &execution_context)
        .await;

    let requests = fixture.child_provider.recorded_requests();
    assert_eq!(
        requests.len(),
        1,
        "valid task did not reach exactly one child Provider request"
    );
    let captured_task = requests[0]
        .messages
        .iter()
        .find_map(|message| match message {
            Message::User(text) => Some(text.as_str()),
            _ => None,
        })
        .expect("child request must contain the delegated User task");
    assert_eq!(captured_task.as_bytes(), task.as_bytes());
    assert_eq!(fixture.child_execute_calls.load(Ordering::SeqCst), 0);
}
