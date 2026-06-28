use crate::agent::message::Message;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::agent::{Agent, AgentStatus};
use crate::cli::{CliError, CliPaths};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::tool::ToolContext;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
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
const MOUSE_SCROLL_LINES: usize = 3;

pub async fn run_tui(paths: CliPaths) -> Result<(), CliError> {
    let config = crate::app::load_config(&paths.user_config, &paths.project_config)?;
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = crate::app::select_provider(&config, credentials)?;
    let provider_name = provider.name().to_string();
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
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
    let agent_handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
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
                        if !is_key_press(key) {
                            continue;
                        }
                        if should_exit(&state, key) {
                            break;
                        }
                        if !handle_scroll_key(&mut terminal, &mut state, key, &theme)? {
                            state.on_key_with_interrupt(key, &input_tx, &interrupt_tx);
                            if state.should_exit {
                                break;
                            }
                        }
                    }
                    Some(Ok(Event::Mouse(mouse))) => {
                        handle_scroll_mouse(&mut terminal, &mut state, mouse, &theme)?;
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
    if !is_key_press(key) {
        return false;
    }

    if state.pending_permission.is_some() {
        return false;
    }

    if key.code == KeyCode::Esc {
        return !state.phase.is_running();
    }

    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn handle_scroll_key(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    key: KeyEvent,
    theme: &theme::Theme,
) -> Result<bool, CliError> {
    if !is_key_press(key) {
        return Ok(false);
    }

    let scroll = match key.code {
        KeyCode::PageUp => app::AppState::page_up,
        KeyCode::PageDown => app::AppState::page_down,
        _ => return Ok(false),
    };
    apply_scroll(terminal, state, theme, scroll)?;
    Ok(true)
}

fn handle_scroll_mouse(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    mouse: MouseEvent,
    theme: &theme::Theme,
) -> Result<bool, CliError> {
    let scroll = match mouse.kind {
        MouseEventKind::ScrollUp => scroll_up_lines,
        MouseEventKind::ScrollDown => scroll_down_lines,
        _ => return Ok(false),
    };
    apply_scroll(terminal, state, theme, scroll)?;
    Ok(true)
}

fn apply_scroll(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    theme: &theme::Theme,
    scroll: fn(&mut app::AppState, usize, usize),
) -> Result<(), CliError> {
    let size = terminal.terminal_mut().size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let total_lines = render::transcript_line_count(state, theme, area.width as usize);
    let viewport_lines = render::transcript_viewport_height(area, state);
    scroll(state, total_lines, viewport_lines);
    Ok(())
}

fn scroll_up_lines(state: &mut app::AppState, total_lines: usize, viewport_lines: usize) {
    state.scroll_up(total_lines, viewport_lines, MOUSE_SCROLL_LINES);
}

fn scroll_down_lines(state: &mut app::AppState, total_lines: usize, viewport_lines: usize) {
    state.scroll_down(total_lines, viewport_lines, MOUSE_SCROLL_LINES);
}

fn is_key_press(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

pub async fn run_agent_task(
    mut agent: Agent,
    mut input_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    mut interrupt_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    ui_tx: mpsc::UnboundedSender<channel::AgentEvent>,
    ctx: ToolContext,
) {
    while let Some(input) = input_rx.recv().await {
        match input {
            channel::UserInput::SetModel(model) => agent.set_model(model),
            channel::UserInput::Interrupt => {}
            channel::UserInput::Prompt(prompt) => {
                while interrupt_rx.try_recv().is_ok() {}

                let mut history = vec![
                    Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                    Message::User(prompt),
                ];
                let sink = channel::ChannelSink::new(ui_tx.clone());
                let observer = channel::ChannelObserver::new(ui_tx.clone());

                tokio::select! {
                    result = agent.run_observed(&mut history, &ctx, &sink, &observer) => {
                        match result {
                            Ok(_) => {
                                let _ = ui_tx.send(channel::AgentEvent::TurnComplete);
                            }
                            Err(err) => {
                                let _ = ui_tx.send(channel::AgentEvent::Error(error_message(err)));
                            }
                        }
                    }
                    _ = wait_for_interrupt(&mut interrupt_rx) => {
                        let _ = ui_tx.send(channel::AgentEvent::Interrupted);
                        let _ = ui_tx.send(channel::AgentEvent::StatusChanged(AgentStatus::Idle));
                    }
                }
            }
        }
    }
}

async fn wait_for_interrupt(input: &mut mpsc::UnboundedReceiver<channel::UserInput>) {
    loop {
        match input.recv().await {
            Some(channel::UserInput::Interrupt) => break,
            Some(_) => {}
            None => std::future::pending::<()>().await,
        }
    }
}

fn error_message(err: AgentError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::channel::{AgentEvent, ChannelDecider, PermissionRequest, UserInput};
    use super::{run_agent_task, should_exit};
    use crate::agent::message::Message;
    use crate::agent::AgentStatus;
    use crate::app::assemble_agent;
    use crate::config::{AuthType, Config, ProviderConfig, ProviderKind};
    use crate::error::ProviderError;
    use crate::permission::PermissionDecision;
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall,
    };
    use crate::tool::ToolContext;
    use async_trait::async_trait;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::{timeout, Duration};

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

    struct HangingProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for HangingProvider {
        fn name(&self) -> &str {
            "hanging"
        }

        async fn complete(
            &self,
            _req: ModelRequest,
            _sink: &dyn DeltaSink,
        ) -> Result<ModelResponse, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
            let _ = rx.await;
            unreachable!("hanging provider should be cancelled before completion")
        }
    }

    struct FirstCallHangsThenRespondsProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for FirstCallHangsThenRespondsProvider {
        fn name(&self) -> &str {
            "first-call-hangs"
        }

        async fn complete(
            &self,
            _req: ModelRequest,
            sink: &dyn DeltaSink,
        ) -> Result<ModelResponse, ProviderError> {
            let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
            if call_index == 0 {
                let (_tx, rx) = tokio::sync::oneshot::channel::<()>();
                let _ = rx.await;
                unreachable!("first provider call should be cancelled before completion")
            }

            sink.on_text("second done");
            Ok(ModelResponse {
                text: "second done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
            })
        }
    }

    #[test]
    fn should_exit_ignores_non_press_escape_events() {
        let state = super::app::AppState::new();
        let release_escape =
            KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Release);
        let repeat_escape =
            KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Repeat);

        assert!(!should_exit(&state, release_escape));
        assert!(!should_exit(&state, repeat_escape));
    }

    #[test]
    fn should_exit_routes_escape_by_pending_running_or_ready_state() {
        let ready = super::app::AppState::new();
        assert!(should_exit(
            &ready,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));

        let mut running = super::app::AppState::new();
        running.phase = super::app::Phase::CallingModel;
        assert!(!should_exit(
            &running,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));

        let (tx, _rx) = oneshot::channel();
        let mut pending = super::app::AppState::new();
        pending.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: tx,
        }));
        assert!(!should_exit(
            &pending,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
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
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
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

        let handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
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
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
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

        let handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
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
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
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

        let handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
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

    #[tokio::test]
    async fn run_agent_task_interrupts_running_prompt_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let agent = assemble_agent(
            Box::new(HangingProvider {
                calls: calls.clone(),
            }),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
        input_tx
            .send(UserInput::Prompt("hang".to_string()))
            .unwrap();

        match ui_rx.recv().await.expect("calling model status") {
            AgentEvent::StatusChanged(AgentStatus::CallingModel) => {}
            other => panic!("expected StatusChanged(CallingModel), got {other:?}"),
        }
        interrupt_tx.send(UserInput::Interrupt).unwrap();

        let interrupted = timeout(Duration::from_millis(100), async {
            loop {
                if let AgentEvent::Interrupted = ui_rx.recv().await.expect("interrupted event") {
                    break;
                }
            }
        })
        .await;

        if interrupted.is_err() {
            handle.abort();
        }
        assert!(
            interrupted.is_ok(),
            "expected Interrupted event before timeout"
        );
        match ui_rx.recv().await.expect("idle status") {
            AgentEvent::StatusChanged(AgentStatus::Idle) => {}
            other => panic!("expected StatusChanged(Idle), got {other:?}"),
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!handle.is_finished());

        drop(input_tx);
        drop(interrupt_tx);
        handle.abort();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn run_agent_task_interrupt_does_not_consume_queued_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let agent = assemble_agent(
            Box::new(FirstCallHangsThenRespondsProvider {
                calls: calls.clone(),
            }),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(agent, input_rx, interrupt_rx, ui_tx, ctx));
        input_tx
            .send(UserInput::Prompt("first".to_string()))
            .unwrap();
        input_tx
            .send(UserInput::Prompt("second".to_string()))
            .unwrap();

        match ui_rx.recv().await.expect("first calling status") {
            AgentEvent::StatusChanged(AgentStatus::CallingModel) => {}
            other => panic!("expected StatusChanged(CallingModel), got {other:?}"),
        }
        interrupt_tx.send(UserInput::Interrupt).unwrap();

        let saw_second_completion = timeout(Duration::from_millis(250), async {
            let mut saw_interrupted = false;
            let mut saw_second_text = false;
            loop {
                match ui_rx.recv().await.expect("ui event") {
                    AgentEvent::Interrupted => saw_interrupted = true,
                    AgentEvent::TextDelta(text) if text == "second done" => {
                        saw_second_text = true;
                    }
                    AgentEvent::TurnComplete if saw_interrupted && saw_second_text => break,
                    _ => {}
                }
            }
        })
        .await;

        if saw_second_completion.is_err() {
            handle.abort();
        }
        assert!(
            saw_second_completion.is_ok(),
            "expected queued prompt to complete after interrupt"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!handle.is_finished());

        drop(input_tx);
        drop(interrupt_tx);
        handle.abort();
        let _ = handle.await;
    }
}
