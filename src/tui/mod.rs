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
use tokio::time::{Duration, MissedTickBehavior};

pub mod app;
pub mod channel;
pub mod command;
pub mod render;
pub mod terminal;
pub mod theme;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

pub async fn run_tui(paths: CliPaths) -> Result<(), CliError> {
    let config = crate::app::load_config(&paths.user_config, &paths.project_config)?;
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = crate::app::select_provider(&config, credentials)?;
    let provider_name = provider.name().to_string();
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let agent = crate::app::assemble_agent(
        provider,
        &config,
        Box::new(channel::ChannelDecider::new(ui_tx.clone())),
    );
    let cwd = paths.cwd.clone();
    let ctx = ToolContext {
        cwd: cwd.clone(),
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
    };
    let agent_handle = tokio::spawn(run_agent_task(agent, input_rx, ui_tx, ctx));
    let mut terminal = terminal::TerminalGuard::new()?;
    let mut state = app::AppState::with_session(app::SessionSnapshot {
        provider: provider_name,
        model: config.model.clone(),
        max_iterations: config.max_iterations,
        cwd,
        tools: crate::app::default_registry().schemas().len(),
    });
    let mut events = EventStream::new();
    let theme = theme::Theme::midnight();
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(120));
    spinner_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    terminal
        .terminal_mut()
        .draw(|frame| render::render(frame, &state, &theme))?;

    loop {
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(Event::Key(key))) => {
                        if should_exit(&state, key) {
                            break;
                        }
                        if !handle_scroll_key(&mut terminal, &mut state, key, &theme)? {
                            state.on_key(key, &input_tx);
                            if state.should_exit {
                                break;
                            }
                        }
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
            _ = spinner_tick.tick() => {
                state.advance_spinner();
            }
        }

        terminal
            .terminal_mut()
            .draw(|frame| render::render(frame, &state, &theme))?;
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

fn handle_scroll_key(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    key: KeyEvent,
    theme: &theme::Theme,
) -> Result<bool, CliError> {
    let scroll = match key.code {
        KeyCode::PageUp => app::AppState::page_up,
        KeyCode::PageDown => app::AppState::page_down,
        _ => return Ok(false),
    };
    let size = terminal.terminal_mut().size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let total_lines = render::transcript_line_count(state, theme);
    let viewport_lines = render::transcript_viewport_height(area, state);
    scroll(state, total_lines, viewport_lines);
    Ok(true)
}

pub async fn run_agent_task(
    mut agent: Agent,
    mut input_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    ui_tx: mpsc::UnboundedSender<channel::AgentEvent>,
    ctx: ToolContext,
) {
    while let Some(input) = input_rx.recv().await {
        match input {
            channel::UserInput::SetModel(model) => agent.set_model(model),
            channel::UserInput::Prompt(prompt) => {
                let mut history = vec![
                    Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                    Message::User(prompt),
                ];
                let sink = channel::ChannelSink::new(ui_tx.clone());
                let observer = channel::ChannelObserver::new(ui_tx.clone());
                match agent
                    .run_observed(&mut history, &ctx, &sink, &observer)
                    .await
                {
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
    use crate::agent::AgentStatus;
    use crate::app::assemble_agent;
    use crate::config::{AuthType, Config, ProviderConfig, ProviderKind};
    use crate::permission::PermissionDecision;
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
    async fn run_agent_task_forwards_observed_tool_events_without_terminal() {
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
            Box::new(provider),
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

        match ui_rx.recv().await.expect("calling model status") {
            AgentEvent::StatusChanged(AgentStatus::CallingModel) => {}
            other => panic!("expected StatusChanged(CallingModel), got {other:?}"),
        }

        match ui_rx.recv().await.expect("tool call started") {
            AgentEvent::ToolCallStarted {
                id,
                name,
                args,
                readonly,
            } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "write_file");
                assert_eq!(args, json!({ "path": "note.txt", "content": "from tui" }));
                assert!(!readonly);
            }
            other => panic!("expected ToolCallStarted, got {other:?}"),
        }

        match ui_rx.recv().await.expect("waiting permission status") {
            AgentEvent::StatusChanged(AgentStatus::WaitingForPermission) => {}
            other => panic!("expected StatusChanged(WaitingForPermission), got {other:?}"),
        }

        match ui_rx.recv().await.expect("permission event") {
            AgentEvent::PermissionRequired(request) => {
                assert_eq!(request.tool_name, "write_file");
                request.responder.send(PermissionDecision::Allow).unwrap();
            }
            other => panic!("expected PermissionRequired, got {other:?}"),
        }

        match ui_rx.recv().await.expect("executing status") {
            AgentEvent::StatusChanged(AgentStatus::ExecutingTool(name)) => {
                assert_eq!(name, "write_file");
            }
            other => panic!("expected StatusChanged(ExecutingTool), got {other:?}"),
        }

        match ui_rx.recv().await.expect("tool call finished") {
            AgentEvent::ToolCallFinished { id, outcome } => {
                assert_eq!(id, "call-1");
                assert!(!outcome.is_error);
                assert!(outcome.content.contains("note.txt"));
            }
            other => panic!("expected ToolCallFinished, got {other:?}"),
        }

        match ui_rx.recv().await.expect("second model status") {
            AgentEvent::StatusChanged(AgentStatus::CallingModel) => {}
            other => panic!("expected second StatusChanged(CallingModel), got {other:?}"),
        }

        match ui_rx.recv().await.expect("text event") {
            AgentEvent::TextDelta(text) => assert_eq!(text, "done"),
            other => panic!("expected TextDelta, got {other:?}"),
        }

        match ui_rx.recv().await.expect("idle status") {
            AgentEvent::StatusChanged(AgentStatus::Idle) => {}
            other => panic!("expected StatusChanged(Idle), got {other:?}"),
        }

        assert!(matches!(ui_rx.recv().await, Some(AgentEvent::TurnComplete)));
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "from tui"
        );

        drop(input_tx);
        handle.await.unwrap();
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

        loop {
            if let AgentEvent::PermissionRequired(request) =
                ui_rx.recv().await.expect("permission event")
            {
                assert_eq!(request.tool_name, "write_file");
                assert_eq!(
                    request.args,
                    json!({ "path": "note.txt", "content": "from tui" })
                );
                request.responder.send(PermissionDecision::Allow).unwrap();
                break;
            }
        }

        loop {
            if let AgentEvent::TextDelta(text) = ui_rx.recv().await.expect("text event") {
                assert_eq!(text, "done");
                break;
            }
        }

        loop {
            if matches!(ui_rx.recv().await, Some(AgentEvent::TurnComplete)) {
                break;
            }
        }
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

    #[tokio::test]
    async fn run_agent_task_applies_set_model_to_next_prompt_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
        }]));
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
            .send(UserInput::SetModel("tui-next-model".to_string()))
            .unwrap();
        input_tx
            .send(UserInput::Prompt("hello".to_string()))
            .unwrap();

        loop {
            if let Some(AgentEvent::TurnComplete) = ui_rx.recv().await {
                break;
            }
        }

        drop(input_tx);
        handle.await.unwrap();

        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].model, "tui-next-model");
    }
}
