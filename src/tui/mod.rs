use crate::agent::message::Message;
use crate::agent::run_compact_command;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::agent::{Agent, AgentStatus, Compacting};
use crate::cli::{CliError, CliPaths};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::provider::Usage;
use crate::tool::ToolContext;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
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
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let assembled = crate::app::assemble_agent(
        provider,
        &config,
        Box::new(channel::ChannelDecider::new(ui_tx.clone())),
    );
    let compacting = assembled.compacting;
    let agent = assembled.agent;
    let agent_history = Arc::new(Mutex::new(vec![Message::System(
        DEFAULT_SYSTEM_PROMPT.to_string(),
    )]));
    let cwd = paths.cwd.clone();
    let ctx = ToolContext {
        cwd: cwd.clone(),
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
    };
    let agent_handle = tokio::spawn(run_agent_task(
        agent,
        agent_history.clone(),
        compacting,
        input_rx,
        interrupt_rx,
        ui_tx,
        ctx,
    ));
    let mut terminal = terminal::TerminalGuard::new()?;
    let mut state = app::AppState::with_session_and_history(
        app::SessionSnapshot {
            provider: provider_name,
            model: config.model.clone(),
            max_iterations: config.max_iterations,
            cwd,
            tools: crate::app::default_registry().schemas().len(),
        },
        agent_history,
    );
    let mut events = EventStream::new();
    let theme = theme::Theme::midnight();
    let debug_events = debug_events_enabled();
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(120));
    spinner_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut calling_model_started_at: Option<Instant> = None;
    let mut first_token_at: Option<Instant> = None;

    terminal
        .terminal_mut()
        .draw(|frame| render::render(frame, &state, &theme))?;

    loop {
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(event)) => {
                        if debug_events {
                            append_debug_event_line(&debug_event_line(&event));
                        }

                        match event {
                            Event::Key(key) => {
                                if !is_key_press(key) {
                                    continue;
                                }
                                if should_exit(&state, key) {
                                    break;
                                }
                                let scroll_handled = if arrows_route_to_completion(&state, key) {
                                    false
                                } else {
                                    handle_scroll_key(&mut terminal, &mut state, key, &theme)?
                                };
                                if !scroll_handled {
                                    state.on_key_with_interrupt(key, &input_tx, &interrupt_tx);
                                    if state.should_exit {
                                        break;
                                    }
                                }
                            }
                            Event::Mouse(_) => {}
                            Event::Resize(_, _) => {}
                            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
                        }
                    }
                    Some(Err(err)) => return Err(CliError::Io(err.to_string())),
                    None => break,
                }
            }
            event = ui_rx.recv() => {
                match event {
                    Some(event) => {
                        apply_ui_event(
                            &mut state,
                            event,
                            &mut calling_model_started_at,
                            &mut first_token_at,
                        );
                    }
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

fn apply_ui_event(
    state: &mut app::AppState,
    event: channel::AgentEvent,
    calling_model_started_at: &mut Option<Instant>,
    first_token_at: &mut Option<Instant>,
) {
    match &event {
        channel::AgentEvent::StatusChanged(AgentStatus::CallingModel) => {
            *calling_model_started_at = Some(Instant::now());
            *first_token_at = None;
            state.reset_streaming_chars_for_call();
        }
        channel::AgentEvent::TextDelta(text) => {
            if first_token_at.is_none() {
                *first_token_at = Some(Instant::now());
            }
            let elapsed = calling_model_started_at
                .as_ref()
                .map(|start| start.elapsed())
                .unwrap_or(Duration::ZERO);
            state.record_streaming_chars(text.chars().count() as u32, elapsed);
        }
        channel::AgentEvent::Usage {
            input_tokens,
            output_tokens,
        } => {
            let elapsed = first_token_at
                .take()
                .map(|start| {
                    calling_model_started_at.take();
                    start.elapsed()
                })
                .or_else(|| calling_model_started_at.take().map(|start| start.elapsed()))
                .unwrap_or(Duration::ZERO);
            state.record_usage(
                Usage {
                    input_tokens: *input_tokens,
                    output_tokens: *output_tokens,
                },
                elapsed,
            );
        }
        _ => {}
    }
    state.apply(event);
}

fn debug_events_enabled() -> bool {
    std::env::var_os("MYSTERIES_TUI_DEBUG_EVENTS")
        .is_some_and(|value| !value.as_os_str().is_empty())
}

fn append_debug_event_line(line: &str) {
    let path = std::env::temp_dir().join("mysteries-tui-events.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn should_exit(state: &app::AppState, key: KeyEvent) -> bool {
    if !is_key_press(key) {
        return false;
    }

    if state.pending_permission.is_some() {
        return false;
    }

    if state.command_completion.is_some() && key.code == KeyCode::Esc {
        return false;
    }

    if key.code == KeyCode::Esc {
        return !state.phase.is_running();
    }

    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn arrows_route_to_completion(state: &app::AppState, key: KeyEvent) -> bool {
    is_key_press(key)
        && state.command_completion.is_some()
        && matches!(key.code, KeyCode::Up | KeyCode::Down)
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

    let Some(scroll) = scroll_action_for_key(key) else {
        return Ok(false);
    };
    apply_scroll(terminal, state, theme, scroll)?;
    Ok(true)
}

fn scroll_action_for_key(key: KeyEvent) -> Option<fn(&mut app::AppState, usize, usize)> {
    if !is_key_press(key) {
        return None;
    }

    match key.code {
        KeyCode::Up => Some(scroll_up_one_line),
        KeyCode::Down => Some(scroll_down_one_line),
        KeyCode::PageUp => Some(app::AppState::page_up),
        KeyCode::PageDown => Some(app::AppState::page_down),
        KeyCode::Home => Some(app::AppState::scroll_to_top),
        KeyCode::End => Some(app::AppState::scroll_to_bottom),
        _ => None,
    }
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

fn scroll_up_one_line(state: &mut app::AppState, total_lines: usize, viewport_lines: usize) {
    state.scroll_up(total_lines, viewport_lines, 1);
}

fn scroll_down_one_line(state: &mut app::AppState, total_lines: usize, viewport_lines: usize) {
    state.scroll_down(total_lines, viewport_lines, 1);
}

fn debug_event_line(event: &Event) -> String {
    match event {
        Event::Key(key) => format!(
            "event=key code={} kind={:?} modifiers={}",
            debug_key_code(key.code),
            key.kind,
            debug_modifiers(key.modifiers)
        ),
        Event::Mouse(mouse) => format!(
            "event=mouse kind={:?} column={} row={} modifiers={}",
            mouse.kind,
            mouse.column,
            mouse.row,
            debug_modifiers(mouse.modifiers)
        ),
        Event::Paste(text) => format!("event=paste len={}", text.chars().count()),
        Event::Resize(columns, rows) => format!("event=resize columns={columns} rows={rows}"),
        Event::FocusGained => "event=focus_gained".to_string(),
        Event::FocusLost => "event=focus_lost".to_string(),
    }
}

fn debug_modifiers(modifiers: KeyModifiers) -> String {
    if modifiers.is_empty() {
        return "NONE".to_string();
    }

    let mut labels = Vec::new();
    for (flag, label) in [
        (KeyModifiers::SHIFT, "SHIFT"),
        (KeyModifiers::CONTROL, "CONTROL"),
        (KeyModifiers::ALT, "ALT"),
        (KeyModifiers::SUPER, "SUPER"),
        (KeyModifiers::HYPER, "HYPER"),
        (KeyModifiers::META, "META"),
    ] {
        if modifiers.contains(flag) {
            labels.push(label);
        }
    }

    labels.join("|")
}

fn debug_key_code(code: KeyCode) -> String {
    match code {
        KeyCode::Char(_) => "Char(<redacted>)".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::F(number) => format!("F({number})"),
        KeyCode::Null => "Null".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::CapsLock => "CapsLock".to_string(),
        KeyCode::ScrollLock => "ScrollLock".to_string(),
        KeyCode::NumLock => "NumLock".to_string(),
        KeyCode::PrintScreen => "PrintScreen".to_string(),
        KeyCode::Pause => "Pause".to_string(),
        KeyCode::Menu => "Menu".to_string(),
        KeyCode::KeypadBegin => "KeypadBegin".to_string(),
        KeyCode::Media(key) => format!("Media({key:?})"),
        KeyCode::Modifier(modifier) => format!("Modifier({modifier:?})"),
    }
}

fn is_key_press(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

pub async fn run_agent_task(
    mut agent: Agent,
    agent_history: Arc<Mutex<Vec<Message>>>,
    compacting: Option<Compacting>,
    mut input_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    mut interrupt_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    ui_tx: mpsc::UnboundedSender<channel::AgentEvent>,
    ctx: ToolContext,
) {
    while let Some(input) = input_rx.recv().await {
        match input {
            channel::UserInput::SetModel(model) => agent.set_model(model),
            channel::UserInput::Interrupt => {}
            channel::UserInput::Compact => {
                let mut history = agent_history.lock().await;
                let outcome = run_compact_command(compacting.as_ref(), &mut history).await;
                let _ = ui_tx.send(channel::AgentEvent::Notice(outcome.notice));
            }
            channel::UserInput::Prompt(prompt) => {
                while interrupt_rx.try_recv().is_ok() {}

                let mut working = {
                    let mut history = agent_history.lock().await;
                    history.push(Message::User(prompt));
                    history.clone()
                };
                let sink = channel::ChannelSink::new(ui_tx.clone());
                let observer = channel::ChannelObserver::new(ui_tx.clone());

                tokio::select! {
                    result = agent.run_observed(&mut working, &ctx, &sink, &observer) => {
                        *agent_history.lock().await = working;
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
                        *agent_history.lock().await = working;
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
    use super::{
        arrows_route_to_completion, run_agent_task, scroll_action_for_key, should_exit,
        DEFAULT_SYSTEM_PROMPT,
    };
    use crate::agent::message::Message;
    use crate::agent::AgentStatus;
    use crate::app::assemble_agent;
    use crate::config::{
        AuthType, Config, ProviderConfig, ProviderKind, DEFAULT_COMPACT_TRIGGER_RATIO,
        DEFAULT_KEEP_RECENT_TURNS,
    };
    use crate::error::ProviderError;
    use crate::permission::PermissionDecision;
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall,
    };
    use crate::tool::ToolContext;
    use crate::tui::app::CommandCompletion;
    use crate::tui::command::command_metadata;
    use async_trait::async_trait;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    };
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::{mpsc, oneshot, Mutex};
    use tokio::time::{timeout, Duration};

    fn agent_history() -> Arc<Mutex<Vec<Message>>> {
        Arc::new(Mutex::new(vec![Message::System(
            DEFAULT_SYSTEM_PROMPT.to_string(),
        )]))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn apply_scroll_key_for_test(
        state: &mut super::app::AppState,
        key: KeyEvent,
        total_lines: usize,
        viewport_lines: usize,
    ) -> bool {
        let Some(scroll) = scroll_action_for_key(key) else {
            return false;
        };
        scroll(state, total_lines, viewport_lines);
        true
    }

    fn config() -> Config {
        Config {
            provider: ProviderConfig {
                id: String::new(),
                kind: ProviderKind::Mock,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "tui-test-model".to_string(),
            max_iterations: 4,
            timeout_secs: 30,
            model_context_window: None,
            compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
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
                usage: None,
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

    fn state_with_command_completion() -> super::app::AppState {
        let mut state = super::app::AppState::new();
        state.command_completion = Some(CommandCompletion {
            candidates: command_metadata().to_vec(),
            selected: 0,
        });
        state
    }

    #[test]
    fn arrows_route_to_completion_when_popup_active() {
        let with_completion = state_with_command_completion();
        assert!(arrows_route_to_completion(
            &with_completion,
            key(KeyCode::Up)
        ));
        assert!(arrows_route_to_completion(
            &with_completion,
            key(KeyCode::Down)
        ));
        assert!(!arrows_route_to_completion(
            &with_completion,
            key(KeyCode::PageUp)
        ));
        assert!(!arrows_route_to_completion(
            &with_completion,
            key(KeyCode::Home)
        ));

        let without_completion = super::app::AppState::new();
        assert!(!arrows_route_to_completion(
            &without_completion,
            key(KeyCode::Up)
        ));

        let release_up =
            KeyEvent::new_with_kind(KeyCode::Up, KeyModifiers::NONE, KeyEventKind::Release);
        assert!(!arrows_route_to_completion(&with_completion, release_up));
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

        let with_completion = state_with_command_completion();
        assert!(!should_exit(
            &with_completion,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));
    }

    #[test]
    fn scroll_key_routing_maps_line_and_boundary_keys_only_for_press() {
        let mut state = super::app::AppState::new();

        assert!(
            apply_scroll_key_for_test(&mut state, key(KeyCode::Up), 40, 5),
            "Up should be handled as one-line scroll up"
        );
        assert_eq!(state.visible_scroll_offset(40, 5), 34);
        assert_eq!(state.visible_scroll_offset(50, 5), 34);

        assert!(apply_scroll_key_for_test(
            &mut state,
            key(KeyCode::Down),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(state.visible_scroll_offset(50, 5), 45);

        state.scroll_up(40, 5, 10);
        assert!(apply_scroll_key_for_test(
            &mut state,
            key(KeyCode::Home),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert_eq!(state.visible_scroll_offset(50, 5), 0);

        assert!(apply_scroll_key_for_test(
            &mut state,
            key(KeyCode::End),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(state.visible_scroll_offset(50, 5), 45);

        let before = state.visible_scroll_offset(40, 5);
        assert!(!apply_scroll_key_for_test(
            &mut state,
            KeyEvent::new_with_kind(KeyCode::Up, KeyModifiers::NONE, KeyEventKind::Release),
            40,
            5
        ));
        assert!(!apply_scroll_key_for_test(
            &mut state,
            KeyEvent::new_with_kind(KeyCode::End, KeyModifiers::NONE, KeyEventKind::Repeat),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), before);
    }

    #[test]
    fn keyboard_boundary_navigation_reaches_top_and_bottom_without_mouse_events() {
        let mut state = super::app::AppState::new();

        assert!(apply_scroll_key_for_test(
            &mut state,
            key(KeyCode::Home),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert_eq!(
            state.visible_scroll_offset(50, 5),
            0,
            "Home should stop following bottom without relying on mouse events"
        );

        assert!(apply_scroll_key_for_test(
            &mut state,
            key(KeyCode::End),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(
            state.visible_scroll_offset(50, 5),
            45,
            "End should restore bottom following without relying on mouse events"
        );
    }

    #[test]
    fn debug_event_line_formats_known_events_and_redacts_char_payloads() {
        let paste_line = super::debug_event_line(&Event::Paste("secret-prompt-body".into()));
        assert_eq!(paste_line, "event=paste len=18");
        assert!(
            !paste_line.contains("secret") && !paste_line.contains("prompt"),
            "diagnostic output must not record pasted prompt text"
        );

        let mouse_line = super::debug_event_line(&Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 7,
            row: 9,
            modifiers: KeyModifiers::SHIFT,
        }));
        assert_eq!(
            mouse_line,
            "event=mouse kind=ScrollUp column=7 row=9 modifiers=SHIFT"
        );

        let key_line = super::debug_event_line(&Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        )));
        assert_eq!(
            key_line,
            "event=key code=Char(<redacted>) kind=Press modifiers=CONTROL"
        );
        assert!(
            !key_line.contains('x'),
            "diagnostic output must not record prompt text or typed characters"
        );
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
                usage: None,
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider,
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));
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
                usage: None,
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));
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
            usage: None,
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));
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
    async fn run_agent_task_accumulates_history_across_prompts_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![
            ModelResponse {
                text: "first reply".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            },
            ModelResponse {
                text: "second reply".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ]));
        let history = agent_history();
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));

        for prompt in ["round one", "round two"] {
            input_tx
                .send(UserInput::Prompt(prompt.to_string()))
                .unwrap();
            loop {
                if let Some(AgentEvent::TurnComplete) = ui_rx.recv().await {
                    break;
                }
            }
        }

        drop(input_tx);
        handle.await.unwrap();

        let second_request_messages = {
            let recorded = provider.recorded_requests();
            assert_eq!(recorded.len(), 2, "expected two provider calls");
            recorded[1].messages.clone()
        };
        assert!(
            second_request_messages
                .iter()
                .any(|msg| { matches!(msg, Message::User(text) if text == "round one") }),
            "second request should include first prompt user message"
        );
        assert!(
            second_request_messages.iter().any(|msg| {
                matches!(
                    msg,
                    Message::Assistant { text, tool_calls }
                        if text == "first reply" && tool_calls.is_empty()
                )
            }),
            "second request should include first round assistant reply"
        );

        let stored = history.lock().await;
        assert!(stored
            .iter()
            .any(|msg| { matches!(msg, Message::User(text) if text == "round one") }));
        assert!(stored.iter().any(|msg| {
            matches!(
                msg,
                Message::Assistant { text, .. } if text == "first reply"
            )
        }));
        assert!(stored
            .iter()
            .any(|msg| { matches!(msg, Message::User(text) if text == "round two") }));
    }

    #[tokio::test]
    async fn run_agent_task_interrupts_running_prompt_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            Arc::new(HangingProvider {
                calls: calls.clone(),
            }),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));
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
        let assembled = assemble_agent(
            Arc::new(FirstCallHangsThenRespondsProvider {
                calls: calls.clone(),
            }),
            &config(),
            Box::new(ChannelDecider::new(ui_tx.clone())),
        );
        let ctx = ToolContext {
            cwd: temp.path().to_path_buf(),
            max_output_bytes: 4096,
        };

        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            input_rx,
            interrupt_rx,
            ui_tx,
            ctx,
        ));
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
