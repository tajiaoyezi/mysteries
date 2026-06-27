use crate::agent::message::Message;
use crate::agent::Agent;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::cli::{CliError, CliPaths};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::tool::ToolContext;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use tokio::sync::mpsc;

pub mod app;
pub mod channel;
pub mod render;
pub mod terminal;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

pub async fn run_tui(paths: CliPaths) -> Result<(), CliError> {
    let config = crate::app::load_config(&paths.user_config, &paths.project_config)?;
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = crate::app::select_provider(&config, credentials)?;
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let agent = crate::app::assemble_agent(
        provider,
        &config,
        Box::new(channel::ChannelDecider::new(ui_tx.clone())),
    );
    let ctx = ToolContext {
        cwd: paths.cwd,
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
    };
    let agent_handle = tokio::spawn(run_agent_task(agent, input_rx, ui_tx, ctx));
    let mut terminal = terminal::TerminalGuard::new()?;
    let mut state = app::AppState::new();
    let mut events = EventStream::new();

    terminal
        .terminal_mut()
        .draw(|frame| render::render(frame, &state))?;

    loop {
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(Event::Key(key))) => {
                        if should_exit(&state, key) {
                            break;
                        }
                        state.on_key(key, &input_tx);
                    }
                    Some(Ok(Event::Resize(_, _))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(CliError::Io(err.to_string())),
                    None => break,
                }
            }
            event = ui_rx.recv() => {
                match event {
                    Some(event) => state.apply(event),
                    None => break,
                }
            }
        }

        terminal
            .terminal_mut()
            .draw(|frame| render::render(frame, &state))?;
    }

    drop(input_tx);
    agent_handle.abort();
    let _ = agent_handle.await;

    Ok(())
}

fn should_exit(state: &app::AppState, key: KeyEvent) -> bool {
    if state.pending_permission.is_some() {
        return false;
    }

    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

pub async fn run_agent_task(
    agent: Agent,
    mut input_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    ui_tx: mpsc::UnboundedSender<channel::AgentEvent>,
    ctx: ToolContext,
) {
    while let Some(input) = input_rx.recv().await {
        match input {
            channel::UserInput::Prompt(prompt) => {
                let mut history = vec![
                    Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                    Message::User(prompt),
                ];
                let sink = channel::ChannelSink::new(ui_tx.clone());
                match agent.run(&mut history, &ctx, &sink).await {
                    Ok(_) => {
                        let _ = ui_tx.send(channel::AgentEvent::TurnComplete);
                    }
                    Err(err) => {
                        let _ = ui_tx.send(channel::AgentEvent::Error(error_message(err)));
                    }
                }
            }
        }
    }
}

fn error_message(err: AgentError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::channel::{AgentEvent, ChannelDecider, UserInput};
    use super::run_agent_task;
    use crate::agent::message::Message;
    use crate::app::assemble_agent;
    use crate::config::{AuthType, Config, ProviderConfig, ProviderKind};
    use crate::provider::mock::MockProvider;
    use crate::provider::{FinishReason, ModelResponse, ToolCall};
    use crate::tool::ToolContext;
    use serde_json::json;
    use std::fs;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn config() -> Config {
        Config {
            provider: ProviderConfig {
                kind: ProviderKind::Mock,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "tui-test-model".to_string(),
            max_iterations: 4,
            timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn run_agent_task_handles_prompt_permission_and_turn_complete_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            ModelResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "write_file".to_string(),
                    arguments: json!({ "path": "note.txt", "content": "from tui" }),
                }],
                finish_reason: FinishReason::ToolCalls,
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
            },
        ]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let agent = assemble_agent(
            Box::new(provider.clone()),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(agent, input_rx, ui_tx, ctx));
        input_tx
            .send(UserInput::Prompt("write note".to_string()))
            .unwrap();

        let permission = ui_rx.recv().await.expect("permission event");
        match permission {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "write_file");
                assert_eq!(
                    request.args,
                    json!({ "path": "note.txt", "content": "from tui" })
                );
                request
                    .responder
                    .send(crate::permission::PermissionDecision::Allow)
                    .unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        let text = ui_rx.recv().await.expect("text event");
        match text {
            AgentEvent::TextDelta(text) => assert_eq!(text, "done"),
            other => panic!("expected TextDelta, got {other:?}"),
        }

        assert!(matches!(ui_rx.recv().await, Some(AgentEvent::TurnComplete)));
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "from tui"
        );

        drop(input_tx);
        handle.await.unwrap();

        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].model, "tui-test-model");
        assert!(matches!(recorded[0].messages[0], Message::System(_)));
    }
}
