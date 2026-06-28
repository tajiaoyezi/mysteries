use async_trait::async_trait;
use mysteries::agent::message::Message;
use mysteries::app::assemble_agent;
use mysteries::config::{AuthType, Config, ProviderConfig, ProviderKind};
use mysteries::permission::{PermissionDecider, PermissionDecision};
use mysteries::provider::mock::MockProvider;
use mysteries::provider::{DeltaSink, FinishReason, ModelResponse, ToolCall};
use mysteries::tool::{Tool, ToolContext};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

fn config() -> Config {
    Config {
        provider: ProviderConfig {
            kind: ProviderKind::Mock,
            base_url: None,
            auth_type: AuthType::ApiKey,
        },
        model: "e2e-model".to_string(),
        max_iterations: 4,
        timeout_secs: 30,
    }
}

fn ctx(root: &Path) -> ToolContext {
    ToolContext {
        cwd: root.to_path_buf(),
        max_output_bytes: 4096,
    }
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
    let agent = assemble_agent(Box::new(provider.clone()), &config(), Box::new(AllowAll));
    let sink = CaptureSink::new();
    let mut history = vec![
        Message::System("system".to_string()),
        Message::User("write a file".to_string()),
    ];

    let final_text = agent
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
    assert_eq!(recorded[0].tools.len(), 7);
    assert_eq!(recorded[1].messages, history[..4].to_vec());
}
