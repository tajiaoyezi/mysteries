use super::super::{DelegateTaskTool, WorkspaceCanonicalizer, DELEGATE_TASK_NAME};
use crate::agent::message::Message;
use crate::agent::{
    Agent, AgentExecutionScope, AgentRuntime, ExecutionBudget, ExecutionCapabilities, NoopObserver,
};
use crate::error::ProviderError;
use crate::permission::{PermissionCheck, PermissionDecider, PermissionDecision};
use crate::provider::mock::MockProvider;
use crate::provider::{DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall};
use crate::tool::{
    BlockingToolLimiter, PermissionLevel, Tool, ToolContext, ToolExecutionContext, ToolOutcome,
    ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::io;
use std::path::Path;
use std::sync::Arc;

const SUCCESS_PREFIX: &str = "subagent report (untrusted):\n";
const ERROR_PREFIX: &str = "delegate_task failed: ";

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
            id: "child-call".to_string(),
            name: name.to_string(),
            arguments: json!({}),
        }],
        finish_reason: FinishReason::ToolCalls,
        usage: None,
        thinking: Vec::new(),
    }
}

fn parent_scope(max_iterations: u32, remaining_child_depth: u32) -> AgentExecutionScope {
    AgentExecutionScope::root(
        ExecutionBudget::new(max_iterations, None, remaining_child_depth),
        ExecutionCapabilities::try_new([DELEGATE_TASK_NAME], [PermissionLevel::ReadOnly]).unwrap(),
    )
}

fn delegate_with_dependencies(
    provider: Arc<dyn Provider>,
    canonicalizer: WorkspaceCanonicalizer,
) -> DelegateTaskTool {
    DelegateTaskTool::with_dependencies(
        AgentRuntime::new(provider, "child-model".to_string()),
        ToolRegistry::new(),
        canonicalizer,
        BlockingToolLimiter::new(4),
    )
}

fn delegate_with_provider(provider: Arc<dyn Provider>) -> DelegateTaskTool {
    delegate_with_dependencies(
        provider,
        Arc::new(|path: &Path| std::fs::canonicalize(path)),
    )
}

async fn invoke_scoped(
    tool: &DelegateTaskTool,
    args: Value,
    scope: &AgentExecutionScope,
    cwd: &Path,
    max_output_bytes: usize,
) -> ToolOutcome {
    let context = ToolContext {
        cwd: cwd.to_path_buf(),
        max_output_bytes,
    };
    let observer = NoopObserver;
    tool.execute_scoped(
        args,
        &ToolExecutionContext {
            tool: &context,
            scope,
            observer: &observer,
            read_root: None,
        },
    )
    .await
}

async fn success_outcome(report: &str, max_output_bytes: usize) -> ToolOutcome {
    let provider = Arc::new(MockProvider::new(vec![final_response(report)]));
    let tool = delegate_with_provider(provider);
    let workspace = tempfile::tempdir().unwrap();
    invoke_scoped(
        &tool,
        json!({ "task": "inspect workspace" }),
        &parent_scope(8, 1),
        workspace.path(),
        max_output_bytes,
    )
    .await
}

async fn root_error_outcome(reason: &str, max_output_bytes: usize) -> ToolOutcome {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let reason = reason.to_string();
    let canonicalizer: WorkspaceCanonicalizer =
        Arc::new(move |_path| Err(io::Error::other(reason.clone())));
    let tool = delegate_with_dependencies(provider, canonicalizer);
    let workspace = tempfile::tempdir().unwrap();
    invoke_scoped(
        &tool,
        json!({ "task": "inspect workspace" }),
        &parent_scope(8, 1),
        workspace.path(),
        max_output_bytes,
    )
    .await
}

fn assert_exact_error(outcome: ToolOutcome, reason: &str) {
    assert_eq!(
        outcome,
        ToolOutcome {
            content: format!("{ERROR_PREFIX}{reason}"),
            is_error: true,
            truncated: false,
            exit: None,
        }
    );
}

fn utf8_prefix(raw: &str, max_output_bytes: usize) -> &str {
    let mut boundary = raw.len().min(max_output_bytes);
    while !raw.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &raw[..boundary]
}

fn assert_bounded(outcome: ToolOutcome, raw: &str, max_output_bytes: usize, is_error: bool) {
    assert_eq!(outcome.content, utf8_prefix(raw, max_output_bytes));
    assert!(raw.starts_with(&outcome.content));
    assert!(outcome.content.len() <= max_output_bytes);
    assert_eq!(outcome.truncated, raw.len() > max_output_bytes);
    assert_eq!(outcome.is_error, is_error);
    assert_eq!(outcome.exit, None);
}

#[tokio::test]
async fn success_uses_exact_untrusted_envelope() {
    let report = "verified child report";
    let outcome = success_outcome(report, 4096).await;

    assert_eq!(
        outcome,
        ToolOutcome {
            content: format!("{SUCCESS_PREFIX}{report}"),
            is_error: false,
            truncated: false,
            exit: None,
        }
    );
}

#[tokio::test]
async fn empty_final_text_is_an_exact_ordinary_delegate_error() {
    let outcome = success_outcome("", 4096).await;

    assert_exact_error(outcome, "child returned an empty final response");
}

#[tokio::test]
async fn unscoped_execute_is_an_exact_ordinary_delegate_error() {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let tool = delegate_with_provider(provider);
    let workspace = tempfile::tempdir().unwrap();
    let context = ToolContext {
        cwd: workspace.path().to_path_buf(),
        max_output_bytes: 4096,
    };

    let outcome = tool
        .execute(json!({ "task": "inspect workspace" }), &context)
        .await;

    assert_exact_error(outcome, "scoped execution context required");
}

#[tokio::test]
async fn invalid_args_are_an_exact_ordinary_delegate_error() {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let tool = delegate_with_provider(provider);
    let workspace = tempfile::tempdir().unwrap();

    let outcome = invoke_scoped(
        &tool,
        json!({ "task": "valid", "extra": true }),
        &parent_scope(8, 1),
        workspace.path(),
        4096,
    )
    .await;

    assert_exact_error(outcome, "invalid delegate_task arguments");
}

#[tokio::test]
async fn workspace_root_failure_is_an_exact_ordinary_delegate_error() {
    let outcome = root_error_outcome("workspace root unavailable", 4096).await;

    assert_exact_error(outcome, "workspace root unavailable");
}

#[tokio::test]
async fn direct_scoped_derive_failure_is_an_exact_ordinary_delegate_error() {
    let provider = Arc::new(MockProvider::new(Vec::new()));
    let tool = delegate_with_provider(provider);
    let workspace = tempfile::tempdir().unwrap();

    let outcome = invoke_scoped(
        &tool,
        json!({ "task": "inspect workspace" }),
        &parent_scope(8, 0),
        workspace.path(),
        4096,
    )
    .await;

    assert_exact_error(outcome, "child depth budget is exhausted");
}

struct FailingProvider;

#[async_trait]
impl Provider for FailingProvider {
    fn name(&self) -> &str {
        "failing-provider"
    }

    async fn complete(
        &self,
        _request: ModelRequest,
        _sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        Err(ProviderError::Transport(
            "child provider unavailable".to_string(),
        ))
    }
}

#[tokio::test]
async fn provider_failure_is_an_exact_ordinary_delegate_error() {
    let tool = delegate_with_provider(Arc::new(FailingProvider));
    let workspace = tempfile::tempdir().unwrap();

    let outcome = invoke_scoped(
        &tool,
        json!({ "task": "inspect workspace" }),
        &parent_scope(8, 1),
        workspace.path(),
        4096,
    )
    .await;

    assert_exact_error(
        outcome,
        "provider transport error: child provider unavailable",
    );
}

#[tokio::test]
async fn child_agent_failure_is_an_exact_ordinary_delegate_error() {
    let provider = Arc::new(MockProvider::new(vec![
        tool_response("missing-child-tool"),
        final_response(""),
    ]));
    let tool = delegate_with_provider(provider);
    let workspace = tempfile::tempdir().unwrap();

    let outcome = invoke_scoped(
        &tool,
        json!({ "task": "inspect workspace" }),
        &parent_scope(1, 1),
        workspace.path(),
        4096,
    )
    .await;

    assert_exact_error(outcome, "agent loop reached max_iterations limit: 1");
}

#[tokio::test]
async fn success_envelope_truncates_ascii_cjk_and_emoji_at_utf8_boundaries() {
    let report = "ascii-甲-🙂-tail";
    let raw = format!("{SUCCESS_PREFIX}{report}");
    let cjk_start = raw.find('甲').unwrap();
    let emoji_start = raw.find('🙂').unwrap();
    let caps = [
        0,
        SUCCESS_PREFIX.len() - 1,
        SUCCESS_PREFIX.len(),
        SUCCESS_PREFIX.len() + 3,
        cjk_start + 1,
        emoji_start + 2,
        raw.len(),
    ];

    for cap in caps {
        let outcome = success_outcome(report, cap).await;
        assert_bounded(outcome, &raw, cap, false);
    }
}

#[tokio::test]
async fn error_envelope_truncates_ascii_cjk_and_emoji_at_utf8_boundaries() {
    let reason = "ascii-甲-🙂-tail";
    let raw = format!("{ERROR_PREFIX}{reason}");
    let cjk_start = raw.find('甲').unwrap();
    let emoji_start = raw.find('🙂').unwrap();
    let caps = [
        0,
        ERROR_PREFIX.len() - 1,
        ERROR_PREFIX.len(),
        ERROR_PREFIX.len() + 3,
        cjk_start + 1,
        emoji_start + 2,
        raw.len(),
    ];

    for cap in caps {
        let outcome = root_error_outcome(reason, cap).await;
        assert_bounded(outcome, &raw, cap, true);
    }
}

#[tokio::test]
async fn tiny_success_caps_never_expose_child_report_bytes() {
    let report = "SECRET-CHILD-REPORT";
    let raw = format!("{SUCCESS_PREFIX}{report}");

    for cap in [0, 1, SUCCESS_PREFIX.len() - 1, SUCCESS_PREFIX.len()] {
        let outcome = success_outcome(report, cap).await;

        assert_bounded(outcome.clone(), &raw, cap, false);
        assert!(SUCCESS_PREFIX.starts_with(&outcome.content));
        assert!(!outcome.content.contains(report));
    }
}

struct PanicDecider;

#[async_trait]
impl PermissionDecider for PanicDecider {
    async fn decide(&self, _check: PermissionCheck<'_>) -> PermissionDecision {
        panic!("depth rejection must happen before permission")
    }
}

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

#[tokio::test]
async fn depth_zero_hard_dispatch_keeps_scope_error_without_delegate_prefix() {
    let child_provider = Arc::new(MockProvider::new(Vec::new()));
    let delegate = delegate_with_provider(child_provider.clone());
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(delegate)).unwrap();
    let outer_provider = Arc::new(MockProvider::new(vec![
        ModelResponse {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "delegate-call".to_string(),
                name: DELEGATE_TASK_NAME.to_string(),
                arguments: json!({ "task": "must not run" }),
            }],
            finish_reason: FinishReason::ToolCalls,
            usage: None,
            thinking: Vec::new(),
        },
        final_response("outer recovered"),
    ]));
    let agent = Agent::new(
        outer_provider,
        registry,
        Box::new(PanicDecider),
        "outer-model".to_string(),
        2,
    );
    let scope = agent.root_scope();
    let mut history = vec![Message::User("hard call delegate".to_string())];
    let context = ToolContext {
        cwd: std::env::current_dir().unwrap(),
        max_output_bytes: 4096,
    };

    let result = agent
        .run_observed_scoped(&scope, &mut history, &context, &NoopSink, &NoopObserver)
        .await
        .unwrap();

    assert_eq!(result, "outer recovered");
    let content = history
        .iter()
        .find_map(|message| match message {
            Message::ToolResult {
                call_id, content, ..
            } if call_id == "delegate-call" => Some(content.as_str()),
            _ => None,
        })
        .expect("depth-zero hard dispatch must publish its scope violation");
    assert_eq!(
        content,
        "execution scope violation: tool `delegate_task` requires child depth 1, but execution scope has 0 remaining"
    );
    assert!(!content.contains(ERROR_PREFIX));
    assert!(child_provider.recorded_requests().is_empty());
}
