use async_trait::async_trait;
use mysteries::agent::message::Message;
use mysteries::agent::PLAN_MODE_INSTRUCTION;
use mysteries::app::assemble_agent;
use mysteries::config::{
    AuthType, Config, ProviderConfig, ProviderKind, DEFAULT_COMPACT_TRIGGER_RATIO,
    DEFAULT_KEEP_RECENT_TURNS, DEFAULT_THINKING,
};
use mysteries::error::AgentError;
use mysteries::permission::{PermissionDecider, PermissionDecision, PermissionMode};
use mysteries::provider::mock::MockProvider;
use mysteries::provider::{
    DeltaSink, Depth, FinishReason, ModelResponse, ThinkingBlock, ThinkingConfig, ToolCall,
};
use mysteries::tool::ask::{Answer, MockPrompter};
use mysteries::tool::plan::{Plan, PlanApprover, PlanDecision};
use mysteries::tool::{Tool, ToolContext};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// builtin-tools spec: 6 个只读内置 + assemble 注入的 ask_user + submit_plan。
const PLAN_PHASE_TOOL_COUNT: usize = 8;
/// Normal/AcceptEdits: 9 个默认内置 + ask_user,不含 plan_only 的 submit_plan。
const EXEC_PHASE_TOOL_COUNT: usize = 10;

const EDIT_TOOLS: &[&str] = &["write_file", "edit_file", "run_shell"];

struct CaptureSink {
    chunks: Mutex<Vec<String>>,
}

impl CaptureSink {
    fn new() -> Self {
        Self {
            chunks: Mutex::new(Vec::new()),
        }
    }

    fn text(&self) -> String {
        self.chunks.lock().unwrap().join("")
    }
}

impl DeltaSink for CaptureSink {
    fn on_text(&self, text: &str) {
        self.chunks.lock().unwrap().push(text.to_string());
    }
}

struct AllowAll;

#[async_trait]
impl PermissionDecider for AllowAll {
    async fn decide(&self, _call: &ToolCall, _tool: &dyn Tool) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

struct DenyWriteFile;

#[async_trait]
impl PermissionDecider for DenyWriteFile {
    async fn decide(&self, call: &ToolCall, _tool: &dyn Tool) -> PermissionDecision {
        if call.name == "write_file" {
            PermissionDecision::Deny
        } else {
            PermissionDecision::Allow
        }
    }
}

/// tui-shell spec: 批准 plan 时 MUST 翻转共享 PermissionMode(Plan→AcceptEdits)。
struct FlippingPlanApprover {
    mode: Arc<Mutex<PermissionMode>>,
}

#[async_trait]
impl PlanApprover for FlippingPlanApprover {
    async fn approve(&self, _plan: &Plan) -> PlanDecision {
        *self.mode.lock().expect("permission_mode mutex poisoned") = PermissionMode::AcceptEdits;
        PlanDecision::Approve
    }
}

fn config() -> Config {
    config_with_max_iterations(4)
}

fn config_with_max_iterations(max_iterations: u32) -> Config {
    Config {
        provider: ProviderConfig {
            id: String::new(),
            kind: ProviderKind::Mock,
            base_url: None,
            auth_type: AuthType::ApiKey,
        },
        model: "e2e-model".to_string(),
        allowed_commands: Vec::new(),
        max_iterations,
        timeout_secs: 30,
        model_context_window: None,
        compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
        keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
        thinking: DEFAULT_THINKING,
    }
}

fn ctx(root: &Path) -> ToolContext {
    ToolContext {
        cwd: root.to_path_buf(),
        max_output_bytes: 4096,
    }
}

fn tool_response(calls: Vec<ToolCall>) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: calls,
        finish_reason: FinishReason::ToolCalls,
        ..Default::default()
    }
}

fn stop_response(text: &str) -> ModelResponse {
    ModelResponse {
        text: text.to_string(),
        tool_calls: Vec::new(),
        finish_reason: FinishReason::Stop,
        ..Default::default()
    }
}

fn tool_response_with_thinking(calls: Vec<ToolCall>, thinking: Vec<ThinkingBlock>) -> ModelResponse {
    ModelResponse {
        text: String::new(),
        tool_calls: calls,
        finish_reason: FinishReason::ToolCalls,
        thinking,
        ..Default::default()
    }
}

fn submit_plan_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "submit_plan".to_string(),
        arguments: json!({
            "title": "E2E plan",
            "steps": [{
                "description": "写文件",
                "validation": "note.txt 存在"
            }]
        }),
    }
}

fn plan_phase_tool_names(tools: &[mysteries::provider::ToolSchema]) -> Vec<&str> {
    tools.iter().map(|schema| schema.name.as_str()).collect()
}

fn assert_plan_phase_tools_only(tools: &[mysteries::provider::ToolSchema]) {
    let names = plan_phase_tool_names(tools);
    assert_eq!(names.len(), PLAN_PHASE_TOOL_COUNT);
    for forbidden in EDIT_TOOLS {
        assert!(
            !names.contains(forbidden),
            "plan 期不应下发变更类工具 {forbidden}"
        );
    }
    assert!(names.contains(&"submit_plan"));
    assert!(names.contains(&"ask_user"));
}

fn assemble_with_plan_seams(
    provider: Arc<MockProvider>,
    decider: Box<dyn PermissionDecider>,
    mode: Arc<Mutex<PermissionMode>>,
    config: &Config,
) -> mysteries::app::AssembledAgent {
    let mut assembled = assemble_agent(
        provider,
        config,
        decider,
        Some(Box::new(FlippingPlanApprover {
            mode: mode.clone(),
        })),
        Some(Box::new(MockPrompter::new(Answer {
            selected: Vec::new(),
            supplement: None,
        }))),
        None,
    );
    assembled.agent.set_permission_mode(mode);
    assembled
}

#[tokio::test]
async fn assembled_agent_runs_multiturn_tool_flow_offline() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        ModelResponse {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "write_file".to_string(),
                arguments: json!({
                    "path": "note.txt",
                    "content": "created by e2e"
                }),
            }],
            finish_reason: FinishReason::ToolCalls,
            ..Default::default()
        },
        ModelResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            ..Default::default()
        },
    ]));
    let assembled = assemble_agent(
        provider.clone(),
        &config(),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    let sink = CaptureSink::new();
    let mut history = vec![
        Message::System("system".to_string()),
        Message::User("write a file".to_string()),
    ];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "done");
    assert_eq!(sink.text(), "done");
    assert_eq!(
        fs::read_to_string(temp.path().join("note.txt")).unwrap(),
        "created by e2e"
    );
    assert_eq!(history.len(), 5);
    assert_eq!(
        history[3],
        Message::ToolResult {
            call_id: "call-1".to_string(),
            content: format!("wrote {}", temp.path().join("note.txt").display()),
            is_error: false,
        }
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].model, "e2e-model");
    assert_eq!(recorded[0].tools.len(), 9);
    assert_eq!(recorded[1].messages, history[..4].to_vec());
}

#[tokio::test]
async fn assembled_agent_plan_approval_flips_mode_and_executes() {
    let temp = tempfile::tempdir().unwrap();
    let mode = Arc::new(Mutex::new(PermissionMode::Plan));
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![submit_plan_call("call-plan")]),
        tool_response(vec![ToolCall {
            id: "call-write".to_string(),
            name: "write_file".to_string(),
            arguments: json!({
                "path": "planned.txt",
                "content": "after approval"
            }),
        }]),
        stop_response("done"),
    ]));
    let assembled = assemble_with_plan_seams(
        provider.clone(),
        Box::new(AllowAll),
        mode.clone(),
        &config(),
    );
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("execute the plan".to_string())];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "done");
    assert_eq!(*mode.lock().unwrap(), PermissionMode::AcceptEdits);
    assert_eq!(
        fs::read_to_string(temp.path().join("planned.txt")).unwrap(),
        "after approval"
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 3);
    assert_plan_phase_tools_only(&recorded[0].tools);
    assert_eq!(
        recorded[0].messages.first(),
        Some(&Message::System(PLAN_MODE_INSTRUCTION.to_string()))
    );
    assert_eq!(recorded[1].tools.len(), EXEC_PHASE_TOOL_COUNT);
    assert!(plan_phase_tool_names(&recorded[1].tools).contains(&"write_file"));
    assert!(!plan_phase_tool_names(&recorded[1].tools).contains(&"submit_plan"));
    assert_eq!(recorded[2].tools.len(), EXEC_PHASE_TOOL_COUNT);
}

#[tokio::test]
async fn assembled_agent_propagates_thinking_depth_and_round_trips_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let thinking = vec![ThinkingBlock {
        text: "e2e reasoning".to_string(),
        signature: Some("sig-e2e".to_string()),
        redacted: false,
    }];
    let provider = Arc::new(MockProvider::new(vec![
        tool_response_with_thinking(
            vec![ToolCall {
                id: "call-read".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({ "path": "." }),
            }],
            thinking.clone(),
        ),
        stop_response("done"),
    ]));
    let mut assembled = assemble_agent(
        provider.clone(),
        &config(),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    assembled
        .agent
        .set_thinking_depth(Arc::new(Mutex::new(Depth::High)));
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("think then list".to_string())];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "done");
    assert_eq!(
        history[1],
        Message::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "call-read".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({ "path": "." }),
            }],
            thinking: thinking.clone(),
        }
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[0].thinking,
        Some(ThinkingConfig {
            depth: Depth::High,
        })
    );
    assert!(recorded[1].messages.iter().any(|msg| {
        matches!(
            msg,
            Message::Assistant {
                thinking: roundtrip,
                ..
            } if *roundtrip == thinking
        )
    }));
}

#[tokio::test]
async fn assembled_agent_permission_denial_records_error_and_continues() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![ToolCall {
            id: "call-deny".to_string(),
            name: "write_file".to_string(),
            arguments: json!({
                "path": "blocked.txt",
                "content": "never written"
            }),
        }]),
        stop_response("continued after denial"),
    ]));
    let assembled = assemble_agent(
        provider.clone(),
        &config(),
        Box::new(DenyWriteFile),
        None,
        None,
        None,
    );
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("try to write".to_string())];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "continued after denial");
    assert!(!temp.path().join("blocked.txt").exists());
    assert_eq!(
        history[2],
        Message::ToolResult {
            call_id: "call-deny".to_string(),
            content: "user denied tool execution".to_string(),
            is_error: true,
        }
    );
    assert_eq!(provider.recorded_requests().len(), 2);
}

#[tokio::test]
async fn assembled_agent_max_iterations_forces_final_with_tools_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "list_dir".to_string(),
            arguments: json!({ "path": "." }),
        }]),
        tool_response(vec![ToolCall {
            id: "call-2".to_string(),
            name: "list_dir".to_string(),
            arguments: json!({ "path": "." }),
        }]),
        stop_response("forced final"),
    ]));
    let assembled = assemble_agent(
        provider.clone(),
        &config_with_max_iterations(2),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("loop forever".to_string())];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "forced final");
    // User + 2×(Assistant+ToolResult) + forced-final Assistant
    assert_eq!(history.len(), 6);
    assert_eq!(
        history.last(),
        Some(&Message::Assistant {
            text: "forced final".to_string(),
            tool_calls: Vec::new(),
            thinking: Vec::new(),
        })
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 3);
    assert!(!recorded[0].tools.is_empty());
    assert!(!recorded[1].tools.is_empty());
    assert!(
        recorded[2].tools.is_empty(),
        "触顶 forced-final 须禁用 tools"
    );
}

#[tokio::test]
async fn assembled_agent_max_iterations_errors_when_forced_final_is_empty() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![ToolCall {
            id: "call-1".to_string(),
            name: "list_dir".to_string(),
            arguments: json!({ "path": "." }),
        }]),
        stop_response(""),
    ]));
    let assembled = assemble_agent(
        provider.clone(),
        &config_with_max_iterations(1),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("loop".to_string())];

    let err = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap_err();

    assert_eq!(err, AgentError::MaxIterations { limit: 1 });
}

#[tokio::test]
async fn assembled_agent_propagates_tool_error_to_next_model_request() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("dup.txt"), "beta beta").unwrap();
    let provider = Arc::new(MockProvider::new(vec![
        tool_response(vec![ToolCall {
            id: "call-edit".to_string(),
            name: "edit_file".to_string(),
            arguments: json!({
                "path": "dup.txt",
                "old_string": "beta",
                "new_string": "delta"
            }),
        }]),
        stop_response("recovered"),
    ]));
    let assembled = assemble_agent(
        provider.clone(),
        &config(),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    let sink = CaptureSink::new();
    let mut history = vec![Message::User("fix dup.txt".to_string())];

    let final_text = assembled
        .agent
        .run(&mut history, &ctx(temp.path()), &sink)
        .await
        .unwrap();

    assert_eq!(final_text, "recovered");
    assert_eq!(
        fs::read_to_string(temp.path().join("dup.txt")).unwrap(),
        "beta beta"
    );
    assert_eq!(
        history[2],
        Message::ToolResult {
            call_id: "call-edit".to_string(),
            content: "expected exactly one match, found 2".to_string(),
            is_error: true,
        }
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1].messages.iter().any(|msg| {
        matches!(
            msg,
            Message::ToolResult {
                call_id,
                content,
                is_error: true,
            } if call_id == "call-edit"
                && content == "expected exactly one match, found 2"
        )
    }));
}

#[tokio::test]
async fn assembled_agent_set_model_strips_thinking_from_history() {
    let thinking = vec![ThinkingBlock {
        text: "cross-model block".to_string(),
        signature: Some("sig-cross-model".to_string()),
        redacted: false,
    }];
    let provider = Arc::new(MockProvider::new(vec![stop_response("ok")]));
    let mut assembled = assemble_agent(
        provider.clone(),
        &config(),
        Box::new(AllowAll),
        None,
        None,
        None,
    );
    let mut history = vec![
        Message::User("prior turn".to_string()),
        Message::Assistant {
            text: "partial".to_string(),
            tool_calls: vec![ToolCall {
                id: "call-old".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({ "path": "." }),
            }],
            thinking: thinking.clone(),
        },
    ];
    let expected_after_strip = vec![
        Message::User("prior turn".to_string()),
        Message::Assistant {
            text: "partial".to_string(),
            tool_calls: vec![ToolCall {
                id: "call-old".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({ "path": "." }),
            }],
            thinking: Vec::new(),
        },
    ];

    assembled
        .agent
        .set_model("m2".to_string(), &mut history);

    assert_eq!(history, expected_after_strip);

    let sink = CaptureSink::new();
    let _ = assembled
        .agent
        .run(&mut history, &ctx(Path::new(".")), &sink)
        .await
        .unwrap();

    let recorded = provider.recorded_requests();
    assert_eq!(recorded[0].model, "m2");
    assert!(!recorded[0].messages.iter().any(|msg| {
        matches!(
            msg,
            Message::Assistant {
                thinking: blocks,
                ..
            } if !blocks.is_empty()
        )
    }));
}
