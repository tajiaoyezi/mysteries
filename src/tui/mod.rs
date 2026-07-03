use crate::agent::message::Message;
use crate::agent::run_compact_command;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::agent::{Agent, AgentStatus, Compacting};
use crate::app::select_provider;
use crate::cli::{load_config_or_onboard, CliError, CliPaths, StdinAuthPrompter};
use crate::config::{Config, ProviderConfig, ProviderKind, ProviderProfile};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::permission::PermissionMode;
use crate::provider::Usage;
use crate::tool::ToolContext;
use crate::tui::clipboard::{copy_selection, ArboardClipboard, Clipboard};
use crate::tui::selection::{Point, SelectionAction};
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::buffer::Buffer;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, MissedTickBehavior};

pub mod app;
pub mod channel;
pub mod clipboard;
pub mod command;
pub mod input_batch;
pub mod input_buffer;
pub(crate) mod input_layout;
pub mod jump_to_bottom;
pub mod render;
pub mod selection;
pub mod terminal;
pub mod theme;
pub(crate) mod width;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
const EVENT_BATCH_CAP: usize = 1 << 20;
const PASTE_CONTINUATION_GRACE: StdDuration = StdDuration::from_millis(10);
/// 判「本批已像粘贴」的原始事件数阈值:单键仅 2 事件(Press+Release),粘贴多字符 chunk
/// 会一次性 buffer 成 ≥4 事件;取 4 = 不误触打字的下限,且把「首 chunk 仅 2~3 字符」也纳入合批,
/// 减少首字符泄漏(如 `ys`=4 事件)。仍无法区分「首 chunk 恰 1 字符」与单键,该边界残留 1 字符泄漏。真机可调。
const PASTE_COALESCE_MIN_EVENTS: usize = 4;
/// 粘贴合批的 chunk 间桥接窗口(比 lone-enter 续读的 10ms 宽,容 ConPTY chunk 间隔)。真机可调。
const PASTE_COALESCE_GRACE: StdDuration = StdDuration::from_millis(30);
const CANCEL_DOUBLE_TAP: StdDuration = StdDuration::from_millis(600);

pub struct RunAgentTaskConfig {
    pub profiles: BTreeMap<String, ProviderProfile>,
    pub startup_config: Config,
    pub credentials_path: PathBuf,
    pub tool_ctx: ToolContext,
}

pub async fn run_tui(paths: CliPaths) -> Result<(), CliError> {
    let mut prompter = StdinAuthPrompter;
    let config = load_config_or_onboard(&paths, &mut prompter)?;
    let profiles =
        crate::app::provider_profiles_from_paths(&paths.user_config, &paths.project_config)
            .map_err(CliError::from)?;
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = crate::app::select_provider(&config, credentials)?;
    let provider_id = config.provider.id.clone();
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let permission_mode = Arc::new(std::sync::Mutex::new(PermissionMode::Normal));
    let assembled = crate::app::assemble_agent(
        provider,
        &config,
        Box::new(channel::ChannelDecider::new(
            ui_tx.clone(),
            permission_mode.clone(),
        )),
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
    let task_config = RunAgentTaskConfig {
        profiles: profiles.clone(),
        startup_config: config.clone(),
        credentials_path: paths.credentials.clone(),
        tool_ctx: ctx,
    };
    let agent_handle = tokio::spawn(run_agent_task(
        agent,
        agent_history.clone(),
        compacting,
        task_config,
        input_rx,
        interrupt_rx,
        ui_tx,
    ));
    let mut terminal = terminal::TerminalGuard::new()?;
    let mut state = app::AppState::with_session_and_history(
        app::SessionSnapshot {
            provider: provider_id,
            model: config.model.clone(),
            max_iterations: config.max_iterations,
            cwd,
            tools: crate::app::default_registry().schemas().len(),
        },
        agent_history,
    );
    state.provider_profiles = profiles;
    state.permission_mode = permission_mode;
    let mut events = EventStream::new();
    let theme = theme::Theme::midnight();
    let debug_events = debug_events_enabled();
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(120));
    spinner_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut calling_model_started_at: Option<Instant> = None;
    let mut first_token_at: Option<Instant> = None;
    let mut clipboard = ArboardClipboard::new();
    let mut last_frame: Option<Buffer> = None;

    terminal
        .terminal_mut()
        .draw(|frame| render::render(frame, &state, &theme))?;

    loop {
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(ev0)) => {
                        let batch = drain_event_batch(ev0)?;
                        if process_event_batch(
                            batch,
                            EventBatchContext {
                                state: &mut state,
                                terminal: &mut terminal,
                                last_frame: &last_frame,
                                clipboard: &mut clipboard,
                                theme: &theme,
                                input_tx: &input_tx,
                                interrupt_tx: &interrupt_tx,
                                debug_events,
                            },
                        )? {
                            break;
                        }
                    }
                    Some(Err(err)) => return Err(CliError::Io(err.to_string())),
                    None => break,
                }
            }
            event = ui_rx.recv() => {
                match event {
                    Some(event) => {
                        let is_terminal = matches!(
                            event,
                            channel::AgentEvent::TurnComplete
                                | channel::AgentEvent::Interrupted
                                | channel::AgentEvent::Error(_)
                        );
                        apply_ui_event(
                            &mut state,
                            event,
                            &mut calling_model_started_at,
                            &mut first_token_at,
                        );
                        if is_terminal && state.has_queue() {
                            if let Some(prompt) = state.dequeue_next() {
                                let _ = input_tx.send(channel::UserInput::Prompt(prompt));
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = spinner_tick.tick() => {
                state.advance_spinner();
            }
        }

        let completed = terminal
            .terminal_mut()
            .draw(|frame| render::render(frame, &state, &theme))?;
        if state.has_selection() {
            last_frame = Some(completed.buffer.clone());
        } else {
            last_frame = None;
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionKeyAction {
    Copy,
    Clear,
}

fn selection_key_action(state: &app::AppState, key: KeyEvent) -> Option<SelectionKeyAction> {
    if !is_key_press(key) || !state.has_selection() {
        return None;
    }

    if state.pending_permission.is_some() {
        return None;
    }

    if state.models_picker.is_some() && key.code == KeyCode::Esc {
        return None;
    }

    if state.command_completion.is_some() && key.code == KeyCode::Esc {
        return None;
    }

    if key.code == KeyCode::Esc {
        return Some(SelectionKeyAction::Clear);
    }

    (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        .then_some(SelectionKeyAction::Copy)
}

fn handle_selection_key(
    state: &mut app::AppState,
    key: KeyEvent,
    last_frame: Option<&Buffer>,
    clipboard: &mut dyn Clipboard,
) -> bool {
    match selection_key_action(state, key) {
        Some(SelectionKeyAction::Copy) => {
            copy_selection(state, last_frame, clipboard);
            true
        }
        Some(SelectionKeyAction::Clear) => {
            state.clear_selection();
            true
        }
        None => false,
    }
}

fn handle_resize(state: &mut app::AppState) {
    state.clear_selection();
}

/// 两次取消键到达间隔判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAction {
    /// 中断当前轮并推进下一条排队。
    InterruptAndAdvance,
    /// 快速连按:清空所有排队。
    ClearAll,
}

/// gap = 两次取消键到达间隔;gap >= threshold → 第 1 次/隔久;gap < threshold → 快速连按清空。
pub fn cancel_action(gap: StdDuration, threshold: StdDuration) -> CancelAction {
    if gap >= threshold {
        CancelAction::InterruptAndAdvance
    } else {
        CancelAction::ClearAll
    }
}

fn is_queue_cancel_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn handle_queue_cancel_key(
    state: &mut app::AppState,
    key: KeyEvent,
    interrupt_tx: &mpsc::UnboundedSender<channel::UserInput>,
    now: Instant,
) -> bool {
    if !is_key_press(key) || state.pending_permission.is_some() || !state.has_queue() {
        return false;
    }
    if !is_queue_cancel_key(key) {
        return false;
    }

    let gap = state
        .last_cancel_at()
        .map(|t| now.duration_since(t))
        .unwrap_or(StdDuration::MAX);

    match cancel_action(gap, CANCEL_DOUBLE_TAP) {
        CancelAction::InterruptAndAdvance => {
            let _ = interrupt_tx.send(channel::UserInput::Interrupt);
            state.set_last_cancel_at(now);
        }
        CancelAction::ClearAll => {
            state.clear_queue();
            let _ = interrupt_tx.send(channel::UserInput::Interrupt);
        }
    }
    true
}

fn should_exit(state: &app::AppState, key: KeyEvent) -> bool {
    if !is_key_press(key) {
        return false;
    }

    if state.pending_permission.is_some() {
        return false;
    }

    if state.models_picker.is_some() && key.code == KeyCode::Esc {
        return false;
    }

    if state.command_completion.is_some() && key.code == KeyCode::Esc {
        return false;
    }

    if state.has_selection()
        && (key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)))
    {
        return false;
    }

    if state.has_queue()
        && (key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)))
    {
        return false;
    }

    if key.code == KeyCode::Esc {
        return !state.phase.is_running();
    }

    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn arrows_route_to_models_picker(state: &app::AppState, key: KeyEvent) -> bool {
    is_key_press(key)
        && state.models_picker.is_some()
        && matches!(key.code, KeyCode::Up | KeyCode::Down)
}

fn arrows_route_to_completion(state: &app::AppState, key: KeyEvent) -> bool {
    is_key_press(key)
        && state.command_completion.is_some()
        && matches!(key.code, KeyCode::Up | KeyCode::Down)
}

fn handle_mouse_wheel(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    theme: &theme::Theme,
    kind: MouseEventKind,
) -> Result<bool, CliError> {
    let Some(action) = mouse_wheel_scroll_action(kind) else {
        return Ok(false);
    };
    apply_mouse_wheel_scroll(terminal, state, theme, action)?;
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MouseWheelScrollAction {
    Up { lines: usize },
    Down { lines: usize },
}

const MOUSE_WHEEL_SCROLL_LINES: usize = 3;

fn mouse_wheel_scroll_action(kind: MouseEventKind) -> Option<MouseWheelScrollAction> {
    match kind {
        MouseEventKind::ScrollUp => Some(MouseWheelScrollAction::Up {
            lines: MOUSE_WHEEL_SCROLL_LINES,
        }),
        MouseEventKind::ScrollDown => Some(MouseWheelScrollAction::Down {
            lines: MOUSE_WHEEL_SCROLL_LINES,
        }),
        _ => None,
    }
}

fn apply_mouse_wheel_scroll_to_state(
    state: &mut app::AppState,
    total_lines: usize,
    viewport_lines: usize,
    action: MouseWheelScrollAction,
) {
    match action {
        MouseWheelScrollAction::Up { lines } => {
            state.scroll_up(total_lines, viewport_lines, lines);
        }
        MouseWheelScrollAction::Down { lines } => {
            state.scroll_down(total_lines, viewport_lines, lines);
        }
    }
    state.clear_selection();
}

fn mouse_point(column: u16, row: u16) -> Point {
    Point { col: column, row }
}

fn mouse_selection_action(kind: MouseEventKind, column: u16, row: u16) -> Option<SelectionAction> {
    let point = mouse_point(column, row);
    match kind {
        MouseEventKind::Down(MouseButton::Left) => Some(SelectionAction::Press(point)),
        MouseEventKind::Drag(MouseButton::Left) => Some(SelectionAction::Drag(point)),
        MouseEventKind::Up(MouseButton::Left) => Some(SelectionAction::Release(point)),
        _ => None,
    }
}

fn handle_mouse_selection_event(
    state: &mut app::AppState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
    last_frame: Option<&Buffer>,
    clipboard: &mut dyn Clipboard,
) -> bool {
    let Some(action) = mouse_selection_action(kind, column, row) else {
        return false;
    };
    state.apply_selection_action(action);
    if matches!(action, SelectionAction::Release(_)) && state.has_selection() {
        copy_selection(state, last_frame, clipboard);
    }
    true
}

fn apply_mouse_wheel_scroll(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    theme: &theme::Theme,
    action: MouseWheelScrollAction,
) -> Result<(), CliError> {
    let size = terminal.terminal_mut().size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let total_lines = render::transcript_line_count(state, theme, area.width as usize);
    let viewport_lines = render::transcript_viewport_height(area, state);
    apply_mouse_wheel_scroll_to_state(state, total_lines, viewport_lines, action);
    Ok(())
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
        KeyCode::PageUp => Some(app::AppState::page_up),
        KeyCode::PageDown => Some(app::AppState::page_down),
        KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(app::AppState::scroll_to_top)
        }
        KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(app::AppState::scroll_to_bottom)
        }
        _ => None,
    }
}

fn apply_scroll_to_state(
    state: &mut app::AppState,
    total_lines: usize,
    viewport_lines: usize,
    scroll: fn(&mut app::AppState, usize, usize),
) {
    scroll(state, total_lines, viewport_lines);
    state.clear_selection();
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
    apply_scroll_to_state(state, total_lines, viewport_lines, scroll);
    Ok(())
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

fn drain_event_batch(ev0: Event) -> Result<Vec<Event>, CliError> {
    let mut batch = vec![ev0];
    loop {
        while crossterm::event::poll(StdDuration::ZERO).map_err(|e| CliError::Io(e.to_string()))? {
            batch.push(crossterm::event::read().map_err(|e| CliError::Io(e.to_string()))?);
            if batch.len() >= EVENT_BATCH_CAP {
                return Ok(batch);
            }
        }

        if batch.len() >= EVENT_BATCH_CAP {
            break;
        }

        // 粘贴合批:ConPTY 把大粘贴切成多个 chunk 分次投递(每 chunk ~数十事件、chunk 间有小间隔)。
        // 一旦本批已像粘贴(事件数够多),用较大 grace 桥接后续 chunk,把整段并成一个 batch,
        // 使 process_event_batch 的 fold_candidate 能看到整段;否则每 chunk 不足阈值行、永不折叠。
        let grace = if batch.len() >= PASTE_COALESCE_MIN_EVENTS {
            PASTE_COALESCE_GRACE
        } else if input_batch::would_submit_lone_enter(&batch) {
            PASTE_CONTINUATION_GRACE
        } else {
            break;
        };

        if crossterm::event::poll(grace).map_err(|e| CliError::Io(e.to_string()))? {
            let ev = crossterm::event::read().map_err(|e| CliError::Io(e.to_string()))?;
            let is_key = matches!(ev, Event::Key(_));
            batch.push(ev);
            if !is_key {
                break; // 非键事件(鼠标 Moved/Focus/Resize)即收批,防高频事件令合批不退出
            }
        } else {
            break; // 静默超过 grace:粘贴/续读结束
        }
    }
    Ok(batch)
}

struct EventBatchContext<'a> {
    state: &'a mut app::AppState,
    terminal: &'a mut terminal::TerminalGuard,
    last_frame: &'a Option<Buffer>,
    clipboard: &'a mut ArboardClipboard,
    theme: &'a theme::Theme,
    input_tx: &'a mpsc::UnboundedSender<channel::UserInput>,
    interrupt_tx: &'a mpsc::UnboundedSender<channel::UserInput>,
    debug_events: bool,
}

fn process_event_batch(
    batch: Vec<Event>,
    ctx: EventBatchContext<'_>,
) -> Result<bool, CliError> {
    let EventBatchContext {
        state,
        terminal,
        last_frame,
        clipboard,
        theme,
        input_tx,
        interrupt_tx,
        debug_events,
    } = ctx;
    for event in &batch {
        if debug_events {
            append_debug_event_line(&debug_event_line(event));
        }
    }

    let press_keys = input_batch::press_key_events(&batch);
    let intents = input_batch::classify_key_batch(&press_keys);

    // 批级折叠:整批为大段纯粘贴(全文本内容键、≥阈值行)时折叠为占位符并消费整批
    if state.pending_permission.is_none() && state.models_picker.is_none() {
        if let Some(text) = input_batch::fold_candidate(&batch, input_batch::PASTE_FOLD_MIN_LINES)
        {
            state.insert_paste_fold(text);
            return Ok(false);
        }
    }

    let mut press_index = 0usize;
    let mut pending_str = String::new();
    let mut modal_closed_in_batch = false;
    let mut break_loop = false;

    for event in batch {
        match event {
            Event::Key(key) if !is_key_press(key) => continue,
            Event::Key(key) => {
                let intent = intents[press_index];
                press_index += 1;

                if should_exit(state, key) {
                    app::flush_merged_input_chars(state, &mut pending_str);
                    break_loop = true;
                    break;
                }
                if handle_selection_key(state, key, last_frame.as_ref(), clipboard) {
                    app::flush_merged_input_chars(state, &mut pending_str);
                    continue;
                }
                if handle_queue_cancel_key(state, key, interrupt_tx, Instant::now()) {
                    app::flush_merged_input_chars(state, &mut pending_str);
                    continue;
                }
                let scroll_handled = if arrows_route_to_models_picker(state, key)
                    || arrows_route_to_completion(state, key)
                {
                    false
                } else {
                    handle_scroll_key(terminal, state, key, theme)?
                };
                if scroll_handled {
                    app::flush_merged_input_chars(state, &mut pending_str);
                    continue;
                }

                match app::apply_batch_input_key(
                    state,
                    key,
                    intent,
                    &mut modal_closed_in_batch,
                    &mut pending_str,
                    input_tx,
                    interrupt_tx,
                ) {
                    app::ApplyBatchKeyResult::Continue => {}
                    app::ApplyBatchKeyResult::BreakBatch => break,
                }
                if state.should_exit {
                    break_loop = true;
                    break;
                }
            }
            Event::Mouse(me) => {
                app::flush_merged_input_chars(state, &mut pending_str);
                if !handle_mouse_selection_event(
                    state,
                    me.kind,
                    me.column,
                    me.row,
                    last_frame.as_ref(),
                    clipboard,
                ) {
                    handle_mouse_wheel(terminal, state, theme, me.kind)?;
                }
            }
            Event::Resize(_, _) => {
                app::flush_merged_input_chars(state, &mut pending_str);
                handle_resize(state);
            }
            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {
                app::flush_merged_input_chars(state, &mut pending_str);
            }
        }
    }

    app::flush_merged_input_chars(state, &mut pending_str);

    Ok(break_loop)
}

pub async fn run_agent_task(
    mut agent: Agent,
    agent_history: Arc<Mutex<Vec<Message>>>,
    mut compacting: Option<Compacting>,
    task_config: RunAgentTaskConfig,
    mut input_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    mut interrupt_rx: mpsc::UnboundedReceiver<channel::UserInput>,
    ui_tx: mpsc::UnboundedSender<channel::AgentEvent>,
) {
    let RunAgentTaskConfig {
        profiles,
        startup_config,
        credentials_path,
        tool_ctx: ctx,
    } = task_config;
    while let Some(input) = input_rx.recv().await {
        match input {
            channel::UserInput::SetModel(model) => agent.set_model(model),
            channel::UserInput::SetProvider { id, model } => {
                if let Err(notice) = apply_set_provider(
                    &profiles,
                    &startup_config,
                    &credentials_path,
                    &id,
                    &model,
                    &mut agent,
                    compacting.as_mut(),
                ) {
                    let _ = ui_tx.send(channel::AgentEvent::Notice(notice));
                }
            }
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
                    }
                }
            }
        }
    }
}

fn build_credential_chain(credentials_path: &std::path::Path) -> CredentialChain {
    CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(credentials_path.to_path_buf())),
    ])
}

fn apply_set_provider(
    profiles: &BTreeMap<String, ProviderProfile>,
    startup_config: &Config,
    credentials_path: &std::path::Path,
    id: &str,
    model: &str,
    agent: &mut Agent,
    compacting: Option<&mut Compacting>,
) -> Result<(), String> {
    let Some(profile) = profiles.get(id) else {
        return Err(format!("未知 provider: {id}"));
    };

    let credentials = build_credential_chain(credentials_path);
    if profile.kind != ProviderKind::Mock && credentials.resolve(id).is_none() {
        return Err(format!("缺少 provider `{id}` 的凭据,无法切换"));
    }

    let transient = Config {
        provider: ProviderConfig {
            id: profile.id.clone(),
            kind: profile.kind.clone(),
            base_url: profile.base_url.clone(),
            auth_type: profile.auth_type.clone(),
        },
        model: model.to_string(),
        max_iterations: startup_config.max_iterations,
        timeout_secs: startup_config.timeout_secs,
        model_context_window: startup_config.model_context_window,
        compact_trigger_ratio: startup_config.compact_trigger_ratio,
        keep_recent_turns: startup_config.keep_recent_turns,
    };

    let provider = select_provider(&transient, credentials).map_err(|err| err.to_string())?;
    agent.set_provider(provider.clone());
    agent.set_model(model.to_string());
    if let Some(compacting) = compacting {
        compacting.set_provider(provider);
        compacting.set_model(model.to_string());
    }

    Ok(())
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
        apply_mouse_wheel_scroll_to_state, arrows_route_to_completion, cancel_action,
        handle_mouse_selection_event, handle_queue_cancel_key, handle_resize,
        handle_selection_key, run_agent_task, scroll_action_for_key, selection_key_action,
        should_exit, CancelAction, MouseWheelScrollAction, RunAgentTaskConfig,
        SelectionKeyAction, DEFAULT_SYSTEM_PROMPT,
    };
    use crate::agent::message::Message;
    use crate::agent::AgentStatus;
    use crate::app::assemble_agent;
    use crate::config::{
        AuthType, Config, ProviderConfig, ProviderKind, ProviderProfile,
        DEFAULT_COMPACT_TRIGGER_RATIO, DEFAULT_KEEP_RECENT_TURNS,
    };
    use crate::error::ProviderError;
    use crate::permission::{PermissionDecision, PermissionMode};
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ToolCall,
    };
    use crate::tool::ToolContext;
    use crate::tui::app::CommandCompletion;
    use crate::tui::clipboard::Clipboard;
    use crate::tui::command::command_metadata;
    use crate::tui::selection::{Point, SelectionAction};
    use async_trait::async_trait;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant as StdInstant;
    use tokio::sync::{mpsc, oneshot, Mutex};
    use tokio::time::{timeout, Duration};

    fn normal_channel_decider(tx: mpsc::UnboundedSender<AgentEvent>) -> ChannelDecider {
        ChannelDecider::new(tx, Arc::new(std::sync::Mutex::new(PermissionMode::Normal)))
    }

    fn agent_history() -> Arc<Mutex<Vec<Message>>> {
        Arc::new(Mutex::new(vec![Message::System(
            DEFAULT_SYSTEM_PROMPT.to_string(),
        )]))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
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
        super::apply_scroll_to_state(state, total_lines, viewport_lines, scroll);
        true
    }

    fn config() -> Config {
        Config {
            provider: ProviderConfig {
                id: "mock".to_string(),
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

    fn task_hotswap(
        temp: &tempfile::TempDir,
        profiles: BTreeMap<String, ProviderProfile>,
    ) -> RunAgentTaskConfig {
        RunAgentTaskConfig {
            profiles,
            startup_config: config(),
            credentials_path: temp.path().join("credentials"),
            tool_ctx: ToolContext {
                cwd: temp.path().to_path_buf(),
                max_output_bytes: 4096,
            },
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

    struct RecordingClipboard {
        calls: Vec<String>,
    }

    impl RecordingClipboard {
        fn new() -> Self {
            Self { calls: Vec::new() }
        }
    }

    impl Clipboard for RecordingClipboard {
        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.calls.push(text);
            Ok(())
        }
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    fn selection_point(col: u16, row: u16) -> Point {
        Point { col, row }
    }

    fn create_selection(state: &mut super::app::AppState) {
        state.apply_selection_action(SelectionAction::Press(selection_point(0, 0)));
        state.apply_selection_action(SelectionAction::Drag(selection_point(4, 0)));
        state.apply_selection_action(SelectionAction::Release(selection_point(4, 0)));
    }

    fn buffer_with_text(text: &str) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
        buffer.set_string(0, 0, text, Style::default());
        buffer
    }

    // --- 消息排队 Task 4.1(cancel_action 纯函数 · RED) ---

    #[test]
    fn cancel_action_wide_gap_interrupts_and_advances() {
        assert_eq!(
            cancel_action(
                std::time::Duration::from_millis(1000),
                std::time::Duration::from_millis(600),
            ),
            CancelAction::InterruptAndAdvance,
        );
    }

    #[test]
    fn cancel_action_narrow_gap_clears_all() {
        assert_eq!(
            cancel_action(
                std::time::Duration::from_millis(100),
                std::time::Duration::from_millis(600),
            ),
            CancelAction::ClearAll,
        );
    }

    #[test]
    fn cancel_action_boundary_gap_equals_threshold_interrupts_and_advances() {
        assert_eq!(
            cancel_action(
                std::time::Duration::from_millis(600),
                std::time::Duration::from_millis(600),
            ),
            CancelAction::InterruptAndAdvance,
        );
    }

    #[test]
    fn should_exit_does_not_exit_when_queue_nonempty() {
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("queued".to_string());

        assert!(!should_exit(&state, ctrl_c()));
        assert!(!should_exit(&state, key(KeyCode::Esc)));
    }

    #[test]
    fn handle_queue_cancel_key_skips_when_pending_permission() {
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let (perm_tx, _perm_rx) = oneshot::channel();
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("queued".to_string());
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: perm_tx,
        }));

        assert!(!handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            StdInstant::now(),
        ));
        assert!(interrupt_rx.try_recv().is_err());
    }

    #[test]
    fn handle_queue_cancel_key_first_press_interrupts_and_records_time() {
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("first".to_string());
        let now = StdInstant::now();

        assert!(handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            now,
        ));
        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert_eq!(state.last_cancel_at(), Some(now));
        assert!(state.has_queue());
    }

    #[test]
    fn handle_queue_cancel_key_quick_second_press_clears_queue() {
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("a".to_string());
        state.enqueue_prompt("b".to_string());
        let first = StdInstant::now();
        assert!(handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            first,
        ));
        let _ = interrupt_rx.try_recv().unwrap();

        let second = first + std::time::Duration::from_millis(100);
        assert!(handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            second,
        ));
        assert!(!state.has_queue());
        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
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
    fn selection_keys_respect_modal_priority_before_selection() {
        let (tx, _rx) = oneshot::channel();
        let mut pending = super::app::AppState::new();
        create_selection(&mut pending);
        pending.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: tx,
        }));
        assert_eq!(selection_key_action(&pending, ctrl_c()), None);
        assert_eq!(selection_key_action(&pending, key(KeyCode::Esc)), None);
        assert!(!should_exit(&pending, ctrl_c()));
        assert!(!should_exit(&pending, key(KeyCode::Esc)));

        let mut completion = state_with_command_completion();
        create_selection(&mut completion);
        assert_eq!(selection_key_action(&completion, key(KeyCode::Esc)), None);
        assert!(!should_exit(&completion, key(KeyCode::Esc)));
    }

    #[test]
    fn selection_keys_intercept_copy_and_clear_before_exit() {
        let mut selected = super::app::AppState::new();
        create_selection(&mut selected);

        assert_eq!(
            selection_key_action(&selected, ctrl_c()),
            Some(SelectionKeyAction::Copy)
        );
        assert_eq!(
            selection_key_action(&selected, key(KeyCode::Esc)),
            Some(SelectionKeyAction::Clear)
        );
        assert!(!should_exit(&selected, ctrl_c()));
        assert!(!should_exit(&selected, key(KeyCode::Esc)));

        let ready = super::app::AppState::new();
        assert_eq!(selection_key_action(&ready, ctrl_c()), None);
        assert!(should_exit(&ready, ctrl_c()));
        assert!(should_exit(&ready, key(KeyCode::Esc)));
    }

    #[test]
    fn handle_selection_key_copies_or_clears_selection() {
        let buffer = buffer_with_text("hello   ");
        let mut copied = super::app::AppState::new();
        create_selection(&mut copied);
        let mut clipboard = RecordingClipboard::new();

        assert!(handle_selection_key(
            &mut copied,
            ctrl_c(),
            Some(&buffer),
            &mut clipboard
        ));
        assert_eq!(clipboard.calls, vec!["hello".to_string()]);
        assert!(copied.has_selection());

        let mut cleared = super::app::AppState::new();
        create_selection(&mut cleared);
        assert!(handle_selection_key(
            &mut cleared,
            key(KeyCode::Esc),
            Some(&buffer),
            &mut clipboard
        ));
        assert!(!cleared.has_selection());
    }
    #[test]
    fn mouse_wheel_scroll_action_maps_scroll_kinds_and_ignores_others() {
        use super::{mouse_wheel_scroll_action, MouseWheelScrollAction, MOUSE_WHEEL_SCROLL_LINES};
        use crossterm::event::MouseButton;

        assert_eq!(
            mouse_wheel_scroll_action(MouseEventKind::ScrollUp),
            Some(MouseWheelScrollAction::Up {
                lines: MOUSE_WHEEL_SCROLL_LINES,
            })
        );
        assert_eq!(
            mouse_wheel_scroll_action(MouseEventKind::ScrollDown),
            Some(MouseWheelScrollAction::Down {
                lines: MOUSE_WHEEL_SCROLL_LINES,
            })
        );
        assert_eq!(mouse_wheel_scroll_action(MouseEventKind::Moved), None);
        assert_eq!(
            mouse_wheel_scroll_action(MouseEventKind::Down(MouseButton::Left)),
            None
        );
    }

    #[test]
    fn mouse_selection_events_copy_on_release_and_wheel_scroll_clears_selection() {
        let buffer = buffer_with_text("hello   ");
        let mut state = super::app::AppState::new();
        let mut clipboard = RecordingClipboard::new();

        assert!(handle_mouse_selection_event(
            &mut state,
            MouseEventKind::Down(MouseButton::Left),
            0,
            0,
            Some(&buffer),
            &mut clipboard,
        ));
        assert!(state.selection.dragging);
        assert!(handle_mouse_selection_event(
            &mut state,
            MouseEventKind::Drag(MouseButton::Left),
            4,
            0,
            Some(&buffer),
            &mut clipboard,
        ));
        assert!(handle_mouse_selection_event(
            &mut state,
            MouseEventKind::Up(MouseButton::Left),
            4,
            0,
            Some(&buffer),
            &mut clipboard,
        ));
        assert_eq!(clipboard.calls, vec!["hello".to_string()]);
        assert!(state.has_selection());

        let mut scrolled = super::app::AppState::new();
        create_selection(&mut scrolled);
        apply_mouse_wheel_scroll_to_state(
            &mut scrolled,
            40,
            5,
            MouseWheelScrollAction::Down { lines: 3 },
        );
        assert!(!scrolled.has_selection());
    }
    #[test]
    fn scroll_key_routing_maps_page_and_boundary_keys_only_for_press() {
        let mut state = super::app::AppState::new();

        assert!(
            scroll_action_for_key(key(KeyCode::Up)).is_none(),
            "Up is reserved for input history, not transcript scroll"
        );
        assert!(
            scroll_action_for_key(key(KeyCode::Down)).is_none(),
            "Down is reserved for input history, not transcript scroll"
        );
        assert!(scroll_action_for_key(key(KeyCode::Home)).is_none());
        assert!(scroll_action_for_key(key(KeyCode::End)).is_none());

        state.scroll_up(40, 5, 10);
        assert!(apply_scroll_key_for_test(
            &mut state,
            ctrl_key(KeyCode::Home),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert_eq!(state.visible_scroll_offset(50, 5), 0);

        assert!(apply_scroll_key_for_test(
            &mut state,
            ctrl_key(KeyCode::End),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(state.visible_scroll_offset(50, 5), 45);

        let before = state.visible_scroll_offset(40, 5);
        assert!(!apply_scroll_key_for_test(
            &mut state,
            KeyEvent::new_with_kind(KeyCode::PageUp, KeyModifiers::NONE, KeyEventKind::Release),
            40,
            5
        ));
        assert!(!apply_scroll_key_for_test(
            &mut state,
            KeyEvent::new_with_kind(KeyCode::End, KeyModifiers::CONTROL, KeyEventKind::Repeat),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), before);
    }

    #[test]
    fn keyboard_scroll_and_resize_clear_selection() {
        let mut scrolled = super::app::AppState::new();
        create_selection(&mut scrolled);
        assert!(apply_scroll_key_for_test(
            &mut scrolled,
            key(KeyCode::PageUp),
            40,
            5
        ));
        assert!(!scrolled.has_selection());

        let mut resized = super::app::AppState::new();
        create_selection(&mut resized);
        handle_resize(&mut resized);
        assert!(!resized.has_selection());
    }
    #[test]
    fn end_and_ctrl_end_map_to_scroll_to_bottom_and_clear_new_message_count() {
        assert!(scroll_action_for_key(key(KeyCode::End)).is_none());
        let scroll_key = ctrl_key(KeyCode::End);
        assert!(
            scroll_action_for_key(scroll_key).is_some(),
            "expected scroll action for {scroll_key:?}"
        );
        let mut state = super::app::AppState::new();
        state.page_up(40, 5);
        state.new_message_count = 2;

        assert!(apply_scroll_key_for_test(&mut state, scroll_key, 40, 5));
        assert!(state.follows_bottom());
        assert_eq!(state.new_message_count, 0);
    }

    #[test]
    fn keyboard_boundary_navigation_reaches_top_and_bottom_without_mouse_events() {
        let mut state = super::app::AppState::new();

        assert!(apply_scroll_key_for_test(
            &mut state,
            ctrl_key(KeyCode::Home),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert_eq!(
            state.visible_scroll_offset(50, 5),
            0,
            "Ctrl+Home should stop following bottom without relying on mouse events"
        );

        assert!(apply_scroll_key_for_test(
            &mut state,
            ctrl_key(KeyCode::End),
            40,
            5
        ));
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(
            state.visible_scroll_offset(50, 5),
            45,
            "Ctrl+End should restore bottom following without relying on mouse events"
        );
    }

    #[test]
    fn bare_home_end_do_not_clear_or_intercept_selection() {
        let mut state = super::app::AppState::new();
        create_selection(&mut state);

        for key_event in [key(KeyCode::Home), key(KeyCode::End)] {
            assert_eq!(selection_key_action(&state, key_event), None);
            assert!(scroll_action_for_key(key_event).is_none());
        }
        assert!(state.has_selection());
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
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

    #[tokio::test]
    async fn run_agent_task_applies_set_provider_to_next_prompt_without_terminal() {
        let temp = tempfile::tempdir().unwrap();
        let old_provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "old".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "alt".to_string(),
            ProviderProfile {
                id: "alt".to_string(),
                kind: ProviderKind::Mock,
                base_url: None,
                model: "alt-model".to_string(),
                auth_type: AuthType::ApiKey,
            },
        );
        let assembled = assemble_agent(
            old_provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, profiles);
        let history = agent_history();
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "alt".to_string(),
                model: "alt-model".to_string(),
            })
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

        assert!(old_provider.recorded_requests().is_empty());
        let locked = history.lock().await;
        assert!(locked
            .iter()
            .any(|msg| matches!(msg, Message::User(text) if text == "hello")));
    }

    #[tokio::test]
    async fn run_agent_task_set_provider_unknown_id_emits_notice_and_keeps_provider() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "still old".to_string(),
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
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "missing".to_string(),
                model: "m1".to_string(),
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("hello".to_string()))
            .unwrap();

        let mut saw_notice = false;
        loop {
            match ui_rx.recv().await.expect("ui event") {
                AgentEvent::Notice(text) if text.contains("未知 provider") => {
                    saw_notice = true;
                }
                AgentEvent::TurnComplete => break,
                _ => {}
            }
        }

        drop(input_tx);
        handle.await.unwrap();

        assert!(saw_notice);
        assert_eq!(provider.recorded_requests().len(), 1);
        assert_eq!(provider.recorded_requests()[0].model, "tui-test-model");
    }

    #[tokio::test]
    async fn run_agent_task_set_provider_missing_credentials_emits_notice_and_keeps_provider() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "still old".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "wps".to_string(),
            ProviderProfile {
                id: "wps".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: Some("https://ai-kas.kso.net/codeplan/v1".to_string()),
                model: "zhipu/glm-5.2".to_string(),
                auth_type: AuthType::ApiKey,
            },
        );
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = task_hotswap(&temp, profiles);
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "wps".to_string(),
                model: "zhipu/glm-5.2".to_string(),
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("hello".to_string()))
            .unwrap();

        let mut saw_notice = false;
        loop {
            match ui_rx.recv().await.expect("ui event") {
                AgentEvent::Notice(text) if text.contains("凭据") => {
                    saw_notice = true;
                }
                AgentEvent::TurnComplete => break,
                _ => {}
            }
        }

        drop(input_tx);
        handle.await.unwrap();

        assert!(saw_notice);
        assert_eq!(provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn run_agent_task_hotswap_smoke_anthropic_to_wps_from_config_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
active = "anthropic"

[providers.anthropic]
kind = "mock"
model = "claude-opus-4-8"

[providers.wps]
kind = "mock"
model = "zhipu/glm-5.2"
"#,
        )
        .unwrap();
        let credentials_path = temp.path().join("credentials");
        fs::write(
            &credentials_path,
            "anthropic = sk-test-anthropic\nwps = sk-test-wps\n",
        )
        .unwrap();

        let profiles =
            crate::app::provider_profiles_from_paths(&config_path, &config_path).unwrap();
        assert_eq!(profiles.len(), 2);

        let old_provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "old".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            old_provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
        );
        let task_config = RunAgentTaskConfig {
            profiles,
            startup_config: config(),
            credentials_path: credentials_path.clone(),
            tool_ctx: ToolContext {
                cwd: temp.path().to_path_buf(),
                max_output_bytes: 4096,
            },
        };
        let history = agent_history();
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            None,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "wps".to_string(),
                model: "zhipu/glm-5.2".to_string(),
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("after hotswap".to_string()))
            .unwrap();

        loop {
            if let Some(AgentEvent::TurnComplete) = ui_rx.recv().await {
                break;
            }
        }

        drop(input_tx);
        handle.await.unwrap();

        assert!(
            old_provider.recorded_requests().is_empty(),
            "initial provider must not serve post-hotswap prompt"
        );
        let locked = history.lock().await;
        assert!(locked.iter().any(|msg| {
            matches!(msg, Message::Assistant { text, .. } if text == "mock response")
        }));
    }
}
