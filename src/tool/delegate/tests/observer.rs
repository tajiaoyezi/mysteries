use super::super::{DelegateTaskTool, DELEGATE_TASK_NAME};
use crate::agent::{
    AgentExecutionScope, AgentObserver, AgentRuntime, AgentStatus, ExecutionBudget,
    ExecutionCapabilities, RunIdentity,
};
use crate::provider::mock::MockProvider;
use crate::provider::{FinishReason, ModelResponse, ToolCall, Usage};
use crate::tool::fs::ReadFileTool;
use crate::tool::{
    PermissionLevel, Tool, ToolContext, ToolExecutionContext, ToolOutcome, ToolRegistry,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
enum ScopedEvent {
    Status(RunIdentity, AgentStatus),
    Usage(RunIdentity, Usage),
    Started(RunIdentity, String, String, Value),
    Finished(RunIdentity, String, bool),
}

#[derive(Default)]
struct RecordingObserver {
    events: Mutex<Vec<ScopedEvent>>,
}

impl RecordingObserver {
    fn events(&self) -> Vec<ScopedEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentObserver for RecordingObserver {
    fn on_scoped_status(&self, identity: &RunIdentity, status: AgentStatus) {
        self.events
            .lock()
            .unwrap()
            .push(ScopedEvent::Status(*identity, status));
    }

    fn on_scoped_usage(&self, identity: &RunIdentity, usage: &Usage) {
        self.events
            .lock()
            .unwrap()
            .push(ScopedEvent::Usage(*identity, usage.clone()));
    }

    fn on_scoped_tool_call_started(
        &self,
        identity: &RunIdentity,
        id: &str,
        name: &str,
        args: &Value,
        _readonly: bool,
    ) {
        self.events.lock().unwrap().push(ScopedEvent::Started(
            *identity,
            id.to_string(),
            name.to_string(),
            args.clone(),
        ));
    }

    fn on_scoped_tool_call_finished(
        &self,
        identity: &RunIdentity,
        id: &str,
        outcome: &ToolOutcome,
    ) {
        self.events.lock().unwrap().push(ScopedEvent::Finished(
            *identity,
            id.to_string(),
            outcome.is_error,
        ));
    }
}

fn usage(input_tokens: u32, output_tokens: u32) -> Usage {
    Usage {
        input_tokens,
        output_tokens,
    }
}

fn child_tool_response(usage: Usage) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: vec![ToolCall {
            id: "child-read".to_string(),
            name: "read_file".to_string(),
            arguments: json!({ "path": "note.txt", "limit": 1 }),
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: Some(usage),
        thinking: Vec::new(),
    }
}

fn child_final_response(text: &str, usage: Usage) -> ModelResponse {
    ModelResponse {
        text: text.to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        usage: Some(usage),
        thinking: Vec::new(),
    }
}

#[tokio::test]
async fn delegate_child_observer_reports_full_forced_final_lifecycle_with_child_identity() {
    let first_usage = usage(11, 3);
    let forced_usage = usage(7, 5);
    let provider = Arc::new(MockProvider::new(vec![
        child_tool_response(first_usage.clone()),
        child_final_response("child report", forced_usage.clone()),
    ]));
    let runtime = AgentRuntime::new(provider, "child-model".to_string());
    let mut child_registry = ToolRegistry::new();
    child_registry.register(Box::new(ReadFileTool)).unwrap();
    let delegate = DelegateTaskTool::new(runtime, child_registry);
    let parent = AgentExecutionScope::root(
        ExecutionBudget::new(1, None, 1),
        ExecutionCapabilities::try_new(
            [DELEGATE_TASK_NAME, "read_file"],
            [PermissionLevel::ReadOnly],
        )
        .unwrap(),
    );
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("note.txt"), "child data\n").unwrap();
    let tool_context = ToolContext {
        cwd: temp.path().to_path_buf(),
        max_output_bytes: 4096,
    };
    let observer = RecordingObserver::default();
    let execution_context = ToolExecutionContext {
        tool: &tool_context,
        scope: &parent,
        observer: &observer,
        read_root: None,
    };

    let outcome = delegate
        .execute_scoped(json!({ "task": "read note.txt" }), &execution_context)
        .await;

    assert!(!outcome.is_error, "{outcome:?}");
    let events = observer.events();
    let child_identity = match events.first() {
        Some(ScopedEvent::Status(identity, AgentStatus::CallingModel)) => *identity,
        other => panic!("child lifecycle must start with CallingModel, got {other:?}"),
    };
    assert_ne!(child_identity.run_id(), parent.identity().run_id());
    assert_eq!(
        child_identity.parent_run_id(),
        Some(parent.identity().run_id())
    );
    assert_eq!(
        events,
        vec![
            ScopedEvent::Status(child_identity, AgentStatus::CallingModel),
            ScopedEvent::Usage(child_identity, first_usage),
            ScopedEvent::Started(
                child_identity,
                "child-read".to_string(),
                "read_file".to_string(),
                json!({ "path": "note.txt", "limit": 1 }),
            ),
            ScopedEvent::Status(
                child_identity,
                AgentStatus::ExecutingTool("read_file".to_string()),
            ),
            ScopedEvent::Finished(child_identity, "child-read".to_string(), false),
            ScopedEvent::Status(child_identity, AgentStatus::CallingModel),
            ScopedEvent::Usage(child_identity, forced_usage),
            ScopedEvent::Status(child_identity, AgentStatus::Idle),
        ],
        "forced-final Provider call must remain attributable and observable"
    );
}
