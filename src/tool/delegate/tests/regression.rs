use super::super::{DelegateTaskTool, DELEGATE_TASK_NAME, SUBAGENT_SYSTEM_PROMPT};
use crate::agent::message::Message;
use crate::agent::{Agent, AgentRuntime};
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use crate::tool::fs::ReadFileTool;
use crate::tool::{BlockingToolLimiter, ToolContext, ToolRegistry};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SECRET_MARKER: &str = "DELEGATE_STATIC_ESCAPE_SECRET_5_5";
const ABSOLUTE_ESCAPE_ID: &str = "escape-absolute";
const PARENT_ESCAPE_ID: &str = "escape-parent";
const OUTER_DELEGATE_ID: &str = "outer-delegate";

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

fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    }
}

fn contains_subagent_system(messages: &[Message]) -> bool {
    messages.iter().any(|message| {
        matches!(
            message,
            Message::System(content) if content == SUBAGENT_SYSTEM_PROMPT
        )
    })
}

fn tool_result<'a>(messages: &'a [Message], call_id: &str) -> (&'a str, bool) {
    messages
        .iter()
        .find_map(|message| match message {
            Message::ToolResult {
                call_id: id,
                content,
                is_error,
            } if id == call_id => Some((content.as_str(), *is_error)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing ToolResult for {call_id}"))
}

fn assert_marker_absent(label: &str, messages: &[Message]) {
    let serialized = serde_json::to_string(messages).unwrap();
    assert!(
        !serialized.contains(SECRET_MARKER),
        "{label} leaked the external secret marker: {serialized}"
    );
}

#[tokio::test]
async fn delegate_static_workspace_escapes_never_reach_child_or_parent_messages() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let secret_file = outside.join("secret.txt");
    std::fs::write(&secret_file, SECRET_MARKER).unwrap();
    let absolute_secret = std::fs::canonicalize(&secret_file).unwrap();

    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![call(
            OUTER_DELEGATE_ID,
            DELEGATE_TASK_NAME,
            json!({ "task": "verify workspace escape containment" }),
        )]),
        tool_response(vec![
            call(
                ABSOLUTE_ESCAPE_ID,
                "read_file",
                json!({ "path": absolute_secret.to_string_lossy() }),
            ),
            call(
                PARENT_ESCAPE_ID,
                "read_file",
                json!({ "path": "../outside/secret.txt" }),
            ),
        ]),
        final_response("both workspace escape attempts were blocked"),
        final_response("outer parent completed"),
    ]));
    let runtime = AgentRuntime::new(provider.clone(), "shared-model".to_string());
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ReadFileTool)).unwrap();
    let child_registry = registry.restricted_to(["read_file"]).unwrap();
    registry
        .register(Box::new(DelegateTaskTool::with_dependencies(
            runtime.clone(),
            child_registry,
            Arc::new(|path: &Path| std::fs::canonicalize(path)),
            BlockingToolLimiter::new(4),
        )))
        .unwrap();
    let agent = Agent::with_runtime(runtime, registry, Box::new(DenyAll), 4);
    let scope = agent.product_root_scope();
    let context = ToolContext {
        cwd: workspace,
        max_output_bytes: 16 * 1024,
    };
    let mut history = vec![Message::User("delegate the containment check".to_string())];

    let final_text = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_observed_scoped(
            &scope,
            &mut history,
            &context,
            &NoopSink,
            &crate::agent::NoopObserver,
        ),
    )
    .await
    .expect("delegate escape regression timed out")
    .expect("outer Agent must continue after child escape errors");

    assert_eq!(final_text, "outer parent completed");
    let requests = provider.recorded_requests();
    assert_eq!(
        requests.len(),
        4,
        "expected outer request, two child requests, and outer continuation"
    );
    let child_requests = requests
        .iter()
        .filter(|request| contains_subagent_system(&request.messages))
        .collect::<Vec<_>>();
    let outer_requests = requests
        .iter()
        .filter(|request| !contains_subagent_system(&request.messages))
        .collect::<Vec<_>>();
    assert_eq!(child_requests.len(), 2);
    assert_eq!(outer_requests.len(), 2);

    for call_id in [ABSOLUTE_ESCAPE_ID, PARENT_ESCAPE_ID] {
        let (content, is_error) = tool_result(&child_requests[1].messages, call_id);
        assert!(is_error, "{call_id} escape must fail closed: {content}");
        assert!(
            content.contains("path escapes read root"),
            "{call_id} returned an unexpected containment error: {content}"
        );
        assert!(
            !content.contains(SECRET_MARKER),
            "{call_id} leaked the external secret marker"
        );
    }

    let (delegate_content, delegate_is_error) = tool_result(&history, OUTER_DELEGATE_ID);
    assert!(
        !delegate_is_error,
        "child containment errors should still allow a final delegate report: {delegate_content}"
    );
    assert_eq!(
        delegate_content,
        "subagent report (untrusted):\nboth workspace escape attempts were blocked"
    );
    let (provider_delegate_content, provider_delegate_is_error) =
        tool_result(&outer_requests[1].messages, OUTER_DELEGATE_ID);
    assert_eq!(provider_delegate_content, delegate_content);
    assert_eq!(provider_delegate_is_error, delegate_is_error);

    for (index, request) in child_requests.iter().enumerate() {
        assert_marker_absent(&format!("child request {index}"), &request.messages);
    }
    for (index, request) in outer_requests.iter().enumerate() {
        assert_marker_absent(&format!("outer request {index}"), &request.messages);
    }
    assert_marker_absent("outer history", &history);
    assert!(!delegate_content.contains(SECRET_MARKER));
    assert!(!final_text.contains(SECRET_MARKER));
}
