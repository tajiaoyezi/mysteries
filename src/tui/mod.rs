use crate::agent::message::Message;
use crate::agent::run_compact_command;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::agent::{Agent, AgentStatus, Compacting};
use crate::app::select_provider;
use crate::cli::{load_config_or_onboard, CliError, CliPaths, StdinAuthPrompter};
use crate::config::{Config, ProviderConfig, ProviderKind, ProviderProfile};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::permission::{PermissionMode, PolicyEngine};
use crate::provider::Usage;
use crate::session::{replace_system_head, SessionMeta, SessionStore};
use crate::tool::ToolContext;
use crate::tui::clipboard::{copy_selection, ArboardClipboard, Clipboard};
use crate::tui::selection::{Point, SelectionAction};
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::buffer::Buffer;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration as StdDuration;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
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
pub(crate) mod markdown;
pub mod permission;
pub mod render;
pub mod selection;
pub mod terminal;
pub mod theme;
pub(crate) mod width;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupMode {
    Fresh,
    Resume,
    Continue,
}

pub fn startup_mode(resume: bool, continue_: bool) -> StartupMode {
    if resume {
        StartupMode::Resume
    } else if continue_ {
        StartupMode::Continue
    } else {
        StartupMode::Fresh
    }
}

const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;
const EVENT_BATCH_CAP: usize = 1 << 20;
const PASTE_CONTINUATION_GRACE: StdDuration = StdDuration::from_millis(10);
/// 判「本批已像粘贴」的原始事件数阈值:单键仅 2 事件(Press+Release),粘贴多字符 chunk
/// 会一次性 buffer 成 ≥4 事件;取 4 = 不误触打字的下限,且把「首 chunk 仅 2~3 字符」也纳入合批,
/// 减少首字符泄漏(如 `ys`=4 事件)。仍无法区分「首 chunk 恰 1 字符」与单键,该边界残留 1 字符泄漏。真机可调。
const PASTE_COALESCE_MIN_EVENTS: usize = 4;
/// 粘贴合批的 chunk 间桥接窗口(比 lone-enter 续读的 10ms 宽,容 ConPTY chunk 间隔)。真机可调。
const PASTE_COALESCE_GRACE: StdDuration = StdDuration::from_millis(30);
const PASTE_FAST_TOPUP_ROUNDS: usize = 5;
const CANCEL_DOUBLE_TAP: StdDuration = StdDuration::from_millis(600);
pub const EXIT_DOUBLE_TAP: StdDuration = StdDuration::from_secs(1);

pub struct RunAgentTaskConfig {
    pub profiles: BTreeMap<String, ProviderProfile>,
    pub startup_config: Config,
    pub credentials_path: PathBuf,
    pub tool_ctx: ToolContext,
}

struct SessionStartup {
    meta: SessionMeta,
    history: Vec<Message>,
    transcript: Vec<app::TranscriptBlock>,
    resume_provider: Option<(String, String)>,
    plan: Option<app::ActivePlan>,
}

fn initial_session_snapshot(config: &Config, cwd: PathBuf) -> app::SessionSnapshot {
    app::SessionSnapshot {
        // restore 未确认前，UI 与后续 snapshot 必须反映 Agent 实际使用的 startup provider。
        provider: config.provider.id.clone(),
        model: config.model.clone(),
        max_iterations: config.max_iterations,
        cwd,
        tools: crate::app::default_registry().schemas().len() + 3,
    }
}

fn send_session_provider_restore(
    input_tx: &mpsc::UnboundedSender<channel::UserInput>,
    id: String,
    model: String,
) {
    let _ = input_tx.send(channel::UserInput::SetProvider {
        id,
        model,
        kind: channel::ProviderSwitchKind::SessionRestore,
    });
}

/// 把 `load` 回传的 plan 落进 `current_plan`（纯视觉恢复 seam）。
/// 函数体仅此一句——async history / input_tx / session_meta / transcript 不进 seam。
pub(crate) fn apply_loaded_plan(state: &mut app::AppState, plan: Option<app::ActivePlan>) {
    state.current_plan = plan;
}

const INTERRUPTED_TOOL_CONTENT: &str = "tool call interrupted before completion";
const PREV_SESSION_INTERRUPTED_OUTPUT: &str = "上次会话已中断";

/// 激活前收口旧中断残留：history 按 Assistant 结果组 / occurrence 补 interrupted；
/// transcript 全部 Running → Error /「上次会话已中断」。
pub(crate) fn normalize_loaded_session(
    history: &mut Vec<Message>,
    transcript: &mut [app::TranscriptBlock],
) {
    fill_dangling_tool_results(history, 0);
    for block in transcript.iter_mut() {
        if let app::TranscriptBlock::Tool(card) = block {
            if card.status == app::ToolCardStatus::Running {
                card.status = app::ToolCardStatus::Error;
                card.output = Some(PREV_SESSION_INTERRUPTED_OUTPUT.to_string());
                card.truncated = false;
                card.exit = None;
            }
        }
    }
}

/// 当前 turn 中断后按 occurrence / FIFO 补齐未配对 ToolResult。
pub(crate) fn complete_interrupted_tool_results(
    history: &mut Vec<Message>,
    turn_history_start: usize,
) {
    fill_dangling_tool_results(history, turn_history_start);
}

/// 从 `start` 起扫描：每个 Assistant.tool_calls 为 occurrence 列表，
/// 其后连续 ToolResult 按 id 消费最早未配对项；剩余按原顺序插入结果组末尾。
fn fill_dangling_tool_results(history: &mut Vec<Message>, start: usize) {
    let start = start.min(history.len());
    let mut i = start;
    while i < history.len() {
        let occurrences: Option<Vec<String>> = match &history[i] {
            Message::Assistant { tool_calls, .. } if !tool_calls.is_empty() => {
                Some(tool_calls.iter().map(|c| c.id.clone()).collect())
            }
            _ => None,
        };
        let Some(mut unpaired) = occurrences else {
            i += 1;
            continue;
        };

        let mut j = i + 1;
        while j < history.len() {
            match &history[j] {
                Message::ToolResult { call_id, .. } => {
                    if let Some(pos) = unpaired.iter().position(|id| id == call_id) {
                        unpaired.remove(pos);
                    }
                    j += 1;
                }
                _ => break,
            }
        }

        for id in unpaired {
            history.insert(
                j,
                Message::ToolResult {
                    call_id: id,
                    content: INTERRUPTED_TOOL_CONTENT.to_string(),
                    is_error: true,
                },
            );
            j += 1;
        }
        i = j;
    }
}

/// 两处 activation load site（`--continue` / picker `--resume` hot-swap）共用的 seam。
/// raw `SessionStore::load` 仍保持磁盘 round-trip；normalization 只在此处发生。
#[allow(clippy::type_complexity)]
pub(crate) fn load_session_for_activation(
    store: &SessionStore,
    id: &str,
) -> std::io::Result<(
    SessionMeta,
    Vec<Message>,
    Vec<app::TranscriptBlock>,
    Option<app::ActivePlan>,
)> {
    let (meta, mut history, mut transcript, plan) = store.load(id)?;
    normalize_loaded_session(&mut history, &mut transcript);
    Ok((meta, history, transcript, plan))
}

pub async fn run_tui(paths: CliPaths, mode: StartupMode) -> Result<(), CliError> {
    let mut prompter = StdinAuthPrompter;
    let config = load_config_or_onboard(&paths, &mut prompter)?;
    let profiles =
        crate::app::provider_profiles_from_paths(&paths.user_config, &paths.project_config)
            .map_err(CliError::from)?;
    let store = SessionStore::new(paths.config_dir.join("sessions"));
    let session_startup = prepare_session_startup(&store, &paths, &config, mode)?;
    let mut session_meta = session_startup.meta.clone();
    let resume_provider = session_startup.resume_provider.clone();
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = crate::app::select_provider(&config, credentials)?;
    let (input_tx, input_rx) = mpsc::unbounded_channel();
    let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    let permission_mode = Arc::new(std::sync::Mutex::new(PermissionMode::Normal));
    let thinking_depth = Arc::new(std::sync::Mutex::new(config.thinking));
    let mut assembled = crate::app::assemble_agent(
        provider,
        &config,
        Box::new(channel::ChannelDecider::new(
            ui_tx.clone(),
            permission_mode.clone(),
            PolicyEngine::from_commands(config.allowed_commands.iter()),
            paths.user_config.clone(),
        )),
        Some(Box::new(channel::ChannelPlanApprover::new(
            ui_tx.clone(),
            permission_mode.clone(),
        ))),
        Some(Box::new(channel::ChannelPrompter::new(ui_tx.clone()))),
        Some(Box::new(channel::ChannelProgressReporter::new(
            ui_tx.clone(),
        ))),
    );
    assembled.agent.set_permission_mode(permission_mode.clone());
    assembled.agent.set_thinking_depth(thinking_depth.clone());
    let compacting = assembled.compacting;
    let agent = assembled.agent;
    let cwd = session_startup.meta.cwd.clone();
    let initial_session = initial_session_snapshot(&config, cwd.clone());
    let agent_history = Arc::new(Mutex::new(session_startup.history));
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
    let mut state = app::AppState::with_session_and_history(initial_session, agent_history);
    state.provider_profiles = profiles;
    state.permission_mode = permission_mode;
    state.thinking_depth = thinking_depth;
    state.transcript = session_startup.transcript;
    apply_loaded_plan(&mut state, session_startup.plan);
    if state.thinking_cannot_disable_active() {
        state.transcript.push(app::TranscriptBlock::Notice(
            "该模型思考无法关闭".to_string(),
        ));
    }
    if mode == StartupMode::Resume {
        let summaries = store.list_sessions().map_err(cli_io_error)?;
        if !summaries.is_empty() {
            state.open_session_picker(summaries);
        }
    }
    if let Some((id, model)) = resume_provider {
        send_session_provider_restore(&input_tx, id, model);
    }
    let mut events = EventStream::new();
    let theme = theme::Theme::midnight();
    let debug_events = debug_events_enabled();
    let mut spinner_tick = tokio::time::interval(Duration::from_millis(120));
    spinner_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut calling_model_started_at: Option<Instant> = None;
    let mut first_token_at: Option<Instant> = None;
    let mut clipboard = ArboardClipboard::new();
    let mut last_frame: Option<Buffer> = None;

    draw_frame(&mut terminal, &mut state, &theme, &mut last_frame)?;

    loop {
        let mut skip_frame = false;
        tokio::select! {
            event = events.next() => {
                match event {
                    Some(Ok(ev0)) => {
                        let mut batch = drain_immediate(ev0)?;
                        state.expire_paste_tail(Instant::now());
                        if batch.len() >= EVENT_BATCH_CAP {
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
                        } else if state.paste_tail_active() {
                            let forwarded = process_paste_tail_batch(&mut state, batch, debug_events);
                            if forwarded.is_empty() {
                                skip_frame = true;
                            } else if process_event_batch(
                                forwarded,
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
                            )?
                            {
                                break;
                            }
                        } else if can_try_fast_paste(&state, &batch) {
                            batch = top_up_fast_paste_batch(batch)?;
                            match input_batch::try_fast_paste_decision(&batch, || clipboard.get_text()) {
                                input_batch::FastPasteDecision::Matched(fast) => {
                                    if debug_events {
                                        log_debug_events_with_disposition(&batch, "fast-paste");
                                    }
                                    state.insert_paste_fold(fast.fold_text);
                                    if !fast.tail.is_done() {
                                        state.set_paste_tail(fast.tail, Instant::now());
                                    }
                                }
                                input_batch::FastPasteDecision::Declined(decline) => {
                                    if debug_events {
                                        append_debug_event_line(&debug_fast_paste_decline_line(decline));
                                        flush_debug_event_log();
                                    }
                                    state.set_paste_receiving_hint(true);
                                    draw_frame(&mut terminal, &mut state, &theme, &mut last_frame)?;
                                    state.set_paste_receiving_hint(false);
                                    batch = bridge_event_batch(batch)?;
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
                            }
                        } else {
                            batch = bridge_event_batch(batch)?;
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
                    }
                    Some(Err(err)) => return Err(CliError::Io(err.to_string())),
                    None => break,
                }
            }
            event = ui_rx.recv() => {
                match event {
                    Some(event) => {
                        let is_terminal = terminal_session_event(&event);
                        let reassert_mouse_capture = matches!(
                            &event,
                            channel::AgentEvent::ToolCallFinished { .. }
                        );
                        handle_agent_event(
                            &mut state,
                            event,
                            &mut calling_model_started_at,
                            &mut first_token_at,
                            &input_tx,
                        );
                        if is_terminal {
                            let h = state.agent_history.lock().await;
                            let save_result =
                                write_session_snapshot(&store, &session_meta, &state, &h);
                            drop(h);
                            push_session_save_notice_if_error(&mut state, save_result);
                        }
                        if reassert_mouse_capture {
                            terminal.reassert_mouse_capture()?;
                        }
                    }
                    None => break,
                }
            }
            _ = spinner_tick.tick() => {
                state.advance_spinner();
                state.expire_paste_tail(Instant::now());
            }
        }

        if let Some(id) = state.take_pending_session_switch() {
            activate_session_switch(&store, &id, &mut state, &mut session_meta, &input_tx).await;
        }

        if !skip_frame {
            draw_frame(&mut terminal, &mut state, &theme, &mut last_frame)?;
        }
    }

    drop(input_tx);
    agent_handle.abort();
    let _ = agent_handle.await;

    Ok(())
}

async fn activate_session_switch(
    store: &SessionStore,
    id: &str,
    state: &mut app::AppState,
    session_meta: &mut SessionMeta,
    input_tx: &mpsc::UnboundedSender<channel::UserInput>,
) {
    match load_session_for_activation(store, id) {
        Ok((meta, mut history, transcript, plan)) => {
            replace_system_head(&mut history, DEFAULT_SYSTEM_PROMPT);
            let mut h = state.agent_history.lock().await;
            *h = history;
            drop(h);
            state.transcript = transcript;
            // 保留选中 session 文件 id；provider/model 仅在 ProviderApplied 后提交。
            let provider = meta.provider.clone();
            let model = meta.model.clone();
            *session_meta = meta;
            apply_loaded_plan(state, plan);
            send_session_provider_restore(input_tx, provider, model);
        }
        Err(_) => state
            .transcript
            .push(app::TranscriptBlock::Notice("会话切换失败".to_string())),
    }
}

fn prepare_session_startup(
    store: &SessionStore,
    paths: &CliPaths,
    config: &Config,
    mode: StartupMode,
) -> Result<SessionStartup, CliError> {
    if mode == StartupMode::Continue {
        if let Some(id) = store.latest().map_err(cli_io_error)? {
            let (meta, mut history, transcript, plan) =
                load_session_for_activation(store, &id).map_err(cli_io_error)?;
            replace_system_head(&mut history, DEFAULT_SYSTEM_PROMPT);
            let resume_provider = Some((meta.provider.clone(), meta.model.clone()));
            return Ok(SessionStartup {
                meta,
                history,
                transcript,
                resume_provider,
                plan,
            });
        }
    }

    let meta = SessionMeta {
        id: SessionStore::new_session_id(),
        provider: config.provider.id.clone(),
        model: config.model.clone(),
        created_at: created_at_now(),
        cwd: paths.cwd.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    Ok(SessionStartup {
        meta,
        history: vec![Message::System(DEFAULT_SYSTEM_PROMPT.to_string())],
        transcript: Vec::new(),
        resume_provider: None,
        plan: None,
    })
}

fn created_at_now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
        .to_string()
}

fn write_session_snapshot(
    store: &SessionStore,
    base_meta: &SessionMeta,
    state: &app::AppState,
    history: &[Message],
) -> std::io::Result<()> {
    let mut meta = base_meta.clone();
    meta.provider = state.session.provider.clone();
    meta.model = state.session.model.clone();
    store.write(
        &meta,
        history,
        &state.transcript,
        state.current_plan.as_ref(),
    )
}

fn push_session_save_notice_if_error(state: &mut app::AppState, save_result: std::io::Result<()>) {
    if save_result.is_err() {
        state
            .transcript
            .push(app::TranscriptBlock::Notice("会话保存失败".to_string()));
    }
}

fn cli_io_error(err: std::io::Error) -> CliError {
    CliError::Io(err.to_string())
}

fn terminal_session_event(event: &channel::AgentEvent) -> bool {
    matches!(
        event,
        channel::AgentEvent::TurnComplete
            | channel::AgentEvent::CompactDone
            | channel::AgentEvent::Interrupted
            | channel::AgentEvent::Error(_)
    )
}

/// ui_rx 臂的完整处理:apply 事件 + 三终止事件推进排队(同一调用内,先算 is_terminal 再 apply)。
/// 抽为函数供集成测试直接驱动(推进闸门 = 本函数唯一出口)。
fn handle_agent_event(
    state: &mut app::AppState,
    event: channel::AgentEvent,
    calling_model_started_at: &mut Option<Instant>,
    first_token_at: &mut Option<Instant>,
    input_tx: &mpsc::UnboundedSender<channel::UserInput>,
) {
    let is_terminal = matches!(
        event,
        channel::AgentEvent::TurnComplete
            | channel::AgentEvent::Interrupted
            | channel::AgentEvent::Error(_)
            | channel::AgentEvent::CompactDone
    );
    apply_ui_event(state, event, calling_model_started_at, first_token_at);
    if is_terminal && state.has_queue() {
        if let Some(prompt) = state.dequeue_next() {
            let _ = input_tx.send(channel::UserInput::Prompt(prompt));
        }
    }
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

static DEBUG_EVENT_LOG: OnceLock<Option<StdMutex<BufWriter<File>>>> = OnceLock::new();

fn debug_event_log() -> Option<&'static StdMutex<BufWriter<File>>> {
    DEBUG_EVENT_LOG
        .get_or_init(|| {
            let path = std::env::temp_dir().join("mysteries-tui-events.log");
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
                .map(|file| StdMutex::new(BufWriter::new(file)))
        })
        .as_ref()
}

fn append_debug_event_line(line: &str) {
    if let Some(log) = debug_event_log() {
        if let Ok(mut writer) = log.lock() {
            let _ = writeln!(writer, "{line}");
        }
    }
}

fn flush_debug_event_log() {
    if let Some(log) = debug_event_log() {
        if let Ok(mut writer) = log.lock() {
            let _ = writer.flush();
        }
    }
}

fn draw_frame(
    terminal: &mut terminal::TerminalGuard,
    state: &mut app::AppState,
    theme: &theme::Theme,
    last_frame: &mut Option<Buffer>,
) -> Result<(), CliError> {
    let completed = terminal
        .terminal_mut()
        .draw(|frame| render::render(frame, state, theme))?;
    state.arm_network_permission_after_render(completed.area);
    if state.has_selection() {
        *last_frame = Some(completed.buffer.clone());
    } else {
        *last_frame = None;
    }
    Ok(())
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

    if state.has_pending_dialog() {
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
    state.unarm_network_permission();
}

/// 两次取消键到达间隔判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAction {
    /// 中断当前轮并推进下一条排队。
    InterruptAndAdvance,
    /// 快速连按:清空所有排队。
    ClearAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitIntent {
    Consumed,
    Exit,
}

/// gap = 两次取消键到达间隔;gap >= threshold → 第 1 次/隔久;gap < threshold → 快速连按清空。
pub fn cancel_action(gap: StdDuration, threshold: StdDuration) -> CancelAction {
    if gap >= threshold {
        CancelAction::InterruptAndAdvance
    } else {
        CancelAction::ClearAll
    }
}

pub fn exit_intent_action(gap: StdDuration, threshold: StdDuration) -> ExitIntent {
    if gap < threshold {
        ExitIntent::Exit
    } else {
        ExitIntent::Consumed
    }
}

fn is_queue_cancel_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn is_ctrl_c_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn handle_queue_cancel_key(
    state: &mut app::AppState,
    key: KeyEvent,
    interrupt_tx: &mpsc::UnboundedSender<channel::UserInput>,
    now: Instant,
) -> bool {
    if !is_key_press(key)
        || state.has_pending_dialog()
        || state.models_picker.is_some()
        || state.session_picker.is_some()
        || state.command_completion.is_some()
        || !state.has_queue()
    {
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

fn handle_idle_exit_intent_key(
    state: &mut app::AppState,
    key: KeyEvent,
    now: Instant,
) -> Option<ExitIntent> {
    if !is_key_press(key)
        || !is_ctrl_c_key(key)
        || state.has_pending_dialog()
        || state.models_picker.is_some()
        || state.session_picker.is_some()
        || state.command_completion.is_some()
        || state.has_selection()
        || state.has_queue()
        || state.phase.is_running()
    {
        return None;
    }

    let gap = state
        .last_exit_intent_at()
        .map(|last| now.duration_since(last))
        .unwrap_or(EXIT_DOUBLE_TAP);
    let action = exit_intent_action(gap, EXIT_DOUBLE_TAP);
    if action == ExitIntent::Consumed {
        state.set_last_exit_intent_at(now);
    }
    Some(action)
}

fn should_exit(state: &app::AppState, key: KeyEvent) -> bool {
    if !is_key_press(key) {
        return false;
    }

    if state.has_pending_dialog() {
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

    false
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

fn debug_event_line_with_disposition(event: &Event, disposition: &str) -> String {
    format!("{} disposition={disposition}", debug_event_line(event))
}

fn log_debug_events_with_disposition(batch: &[Event], disposition: &str) {
    for event in batch {
        append_debug_event_line(&debug_event_line_with_disposition(event, disposition));
    }
    flush_debug_event_log();
}

fn debug_fast_paste_decline_line(decline: input_batch::FastPasteDecline) -> String {
    format!(
        "paste-fast decline reason={} rebuilt_chars={} batch_len={}",
        decline.reason.as_str(),
        decline.rebuilt_chars,
        decline.batch_len
    )
}

fn debug_paste_tail_abort_line(matcher: &input_batch::PasteTailMatcher) -> String {
    format!(
        "paste-tail abort streak={} cursor={} normalized_len={}",
        input_batch::PASTE_TAIL_ABORT_MISMATCHES,
        matcher.cursor(),
        matcher.normalized_len()
    )
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

fn drain_immediate(ev0: Event) -> Result<Vec<Event>, CliError> {
    let mut batch = vec![ev0];
    while crossterm::event::poll(StdDuration::ZERO).map_err(|e| CliError::Io(e.to_string()))? {
        batch.push(crossterm::event::read().map_err(|e| CliError::Io(e.to_string()))?);
        if batch.len() >= EVENT_BATCH_CAP {
            return Ok(batch);
        }
    }
    Ok(batch)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchReadOutcome {
    Key,
    NonKey,
    Quiet,
    Cap,
}

fn read_event_batch_round(
    batch: &mut Vec<Event>,
    grace: StdDuration,
) -> Result<BatchReadOutcome, CliError> {
    if crossterm::event::poll(grace).map_err(|e| CliError::Io(e.to_string()))? {
        let ev = crossterm::event::read().map_err(|e| CliError::Io(e.to_string()))?;
        let is_key = matches!(ev, Event::Key(_));
        batch.push(ev);
        if batch.len() >= EVENT_BATCH_CAP {
            return Ok(BatchReadOutcome::Cap);
        }
        if !is_key {
            return Ok(BatchReadOutcome::NonKey);
        }
        while crossterm::event::poll(StdDuration::ZERO).map_err(|e| CliError::Io(e.to_string()))? {
            batch.push(crossterm::event::read().map_err(|e| CliError::Io(e.to_string()))?);
            if batch.len() >= EVENT_BATCH_CAP {
                return Ok(BatchReadOutcome::Cap);
            }
        }
        Ok(BatchReadOutcome::Key)
    } else {
        Ok(BatchReadOutcome::Quiet)
    }
}

fn bridge_event_batch(mut batch: Vec<Event>) -> Result<Vec<Event>, CliError> {
    loop {
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

        match read_event_batch_round(&mut batch, grace)? {
            BatchReadOutcome::Key => {}
            BatchReadOutcome::NonKey => break, // 非键事件(鼠标 Moved/Focus/Resize)即收批,防高频事件令合批不退出
            BatchReadOutcome::Quiet => break,  // 静默超过 grace:粘贴/续读结束
            BatchReadOutcome::Cap => return Ok(batch),
        }
    }
    Ok(batch)
}

fn top_up_fast_paste_batch(mut batch: Vec<Event>) -> Result<Vec<Event>, CliError> {
    for _ in 0..PASTE_FAST_TOPUP_ROUNDS {
        let Some(chars) = fast_paste_rebuilt_chars(&batch) else {
            break;
        };
        if chars >= input_batch::PASTE_FAST_MIN_MATCH_CHARS {
            break;
        }
        match read_event_batch_round(&mut batch, PASTE_COALESCE_GRACE)? {
            BatchReadOutcome::Key => {}
            BatchReadOutcome::NonKey | BatchReadOutcome::Quiet => break,
            BatchReadOutcome::Cap => return Ok(batch),
        }
    }
    Ok(batch)
}

fn fast_paste_rebuilt_chars(batch: &[Event]) -> Option<usize> {
    let keys = input_batch::press_key_events(batch);
    input_batch::rebuild_fast_text(&keys).map(|text| text.chars().count())
}

fn can_try_fast_paste(state: &app::AppState, batch: &[Event]) -> bool {
    batch.len() >= PASTE_COALESCE_MIN_EVENTS
        && !state.has_pending_dialog()
        && state.models_picker.is_none()
        && state.session_picker.is_none()
}

fn process_paste_tail_batch(
    state: &mut app::AppState,
    batch: Vec<Event>,
    debug_events: bool,
) -> Vec<Event> {
    let mut forwarded = Vec::new();
    let mut events = batch.into_iter();
    while let Some(event) = events.next() {
        let Some(tail) = state.paste_tail.as_mut() else {
            forwarded.push(event);
            forwarded.extend(events);
            break;
        };
        let was_aborted = tail.matcher.is_aborted();
        let action = tail.matcher.on_event(&event);
        let done = tail.matcher.is_done();
        let abort_line = (!was_aborted && tail.matcher.is_aborted())
            .then(|| debug_paste_tail_abort_line(&tail.matcher));

        if debug_events {
            if let Some(line) = abort_line {
                append_debug_event_line(&line);
            }
        }
        match action {
            input_batch::TailAction::Drop => {
                if debug_events {
                    append_debug_event_line(&debug_event_line_with_disposition(
                        &event,
                        "tail-drop",
                    ));
                }
                if matches!(event, Event::Key(key) if is_key_press(key)) {
                    state.record_paste_tail_action(input_batch::TailAction::Drop, Instant::now());
                }
            }
            input_batch::TailAction::Forward => forwarded.push(event),
        }

        if done {
            state.clear_paste_tail();
            forwarded.extend(events);
            break;
        }
    }
    if debug_events {
        flush_debug_event_log();
    }
    forwarded
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

fn process_event_batch(batch: Vec<Event>, ctx: EventBatchContext<'_>) -> Result<bool, CliError> {
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
    let terminal_size = terminal.terminal_mut().size()?;
    let terminal_area = ratatui::layout::Rect::new(0, 0, terminal_size.width, terminal_size.height);
    let isolate_network_approval = state.network_permission_needs_input_barrier();
    let batch = if isolate_network_approval {
        isolate_network_approval_events(batch)
    } else {
        batch
    };
    for event in &batch {
        if debug_events {
            append_debug_event_line(&debug_event_line(event));
        }
    }
    if debug_events {
        flush_debug_event_log();
    }

    let press_keys = input_batch::press_key_events(&batch);
    let intents = input_batch::classify_key_batch(&press_keys);

    // 批级折叠:整批为大段纯粘贴(全文本内容键、行/字符任一阈值达标)时折叠为占位符并消费整批
    if !state.has_pending_dialog()
        && state.models_picker.is_none()
        && state.session_picker.is_none()
    {
        if let Some(text) = input_batch::fold_candidate(
            &batch,
            input_batch::PASTE_FOLD_MIN_LINES,
            input_batch::PASTE_FOLD_MIN_CHARS,
        ) {
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

                if let Some(control) = handle_session_picker_batch_key(state, key, &mut pending_str)
                {
                    if control == app::ApplyBatchKeyResult::BreakBatch {
                        break;
                    }
                    continue;
                }
                if let Some(action) = handle_idle_exit_intent_key(state, key, Instant::now()) {
                    app::flush_merged_input_chars(state, &mut pending_str);
                    if action == ExitIntent::Exit {
                        break_loop = true;
                        break;
                    }
                    continue;
                }
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
                    app::BatchInputContext {
                        input_tx,
                        interrupt_tx,
                        terminal_area: Some(terminal_area),
                    },
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
    if isolate_network_approval {
        state.complete_network_permission_input_barrier();
    }

    Ok(break_loop)
}

pub(crate) fn handle_session_picker_batch_key(
    state: &mut app::AppState,
    key: KeyEvent,
    pending_str: &mut String,
) -> Option<app::ApplyBatchKeyResult> {
    state.session_picker.as_ref()?;

    app::flush_merged_input_chars(state, pending_str);
    state.handle_session_picker_key(key);
    Some(if state.pending_session_switch().is_some() {
        // 选中 session 后立即终止本批，避免尾随键在 activation 前污染 input / 提交 Prompt。
        app::ApplyBatchKeyResult::BreakBatch
    } else {
        app::ApplyBatchKeyResult::Continue
    })
}

fn isolate_network_approval_events(batch: Vec<Event>) -> Vec<Event> {
    batch
        .into_iter()
        .filter(|event| {
            !matches!(event, Event::Paste(_))
                && !matches!(
                    event,
                    Event::Key(key)
                        if is_key_press(*key)
                            && matches!(
                                key.code,
                                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
                            )
                )
        })
        .collect()
}

pub async fn run_agent_task(
    mut agent: Agent,
    agent_history: Arc<Mutex<Vec<Message>>>,
    mut compacting: Compacting,
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
            channel::UserInput::SetModel(model) => {
                let mut history = agent_history.lock().await;
                agent.set_model(model, &mut history);
            }
            channel::UserInput::SetProvider { id, model, kind } => {
                let mut history = agent_history.lock().await;
                match apply_set_provider(
                    &profiles,
                    &startup_config,
                    &credentials_path,
                    &id,
                    &model,
                    &mut agent,
                    &mut compacting,
                    &mut history,
                    kind,
                ) {
                    Ok(()) => {
                        let _ = ui_tx.send(channel::AgentEvent::ProviderApplied { id, model });
                    }
                    Err(notice) => {
                        let _ = ui_tx.send(channel::AgentEvent::Notice(notice));
                    }
                }
            }
            channel::UserInput::Interrupt => {}
            channel::UserInput::Compact => {
                let mut history = agent_history.lock().await;
                let outcome = run_compact_command(&compacting, &mut history).await;
                let _ = ui_tx.send(channel::AgentEvent::Notice(outcome.notice));
                // 成功/失败都要收场:置回 Ready 并驱动排队推进。
                let _ = ui_tx.send(channel::AgentEvent::CompactDone);
            }
            channel::UserInput::Prompt(prompt) => {
                while interrupt_rx.try_recv().is_ok() {}

                let (mut working, turn_history_start) = {
                    let mut history = agent_history.lock().await;
                    history.push(Message::User(prompt));
                    let turn_history_start = history.len() - 1;
                    (history.clone(), turn_history_start)
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
                        // drop run future 后补齐本 turn 未配对 occurrence，再只发 Interrupted。
                        complete_interrupted_tool_results(&mut working, turn_history_start);
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

#[allow(clippy::too_many_arguments)]
fn apply_set_provider(
    profiles: &BTreeMap<String, ProviderProfile>,
    startup_config: &Config,
    credentials_path: &std::path::Path,
    id: &str,
    model: &str,
    agent: &mut Agent,
    compacting: &mut Compacting,
    history: &mut [Message],
    kind: channel::ProviderSwitchKind,
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
        allowed_commands: startup_config.allowed_commands.clone(),
        max_iterations: startup_config.max_iterations,
        timeout_secs: startup_config.timeout_secs,
        model_context_window: startup_config.model_context_window,
        compact_trigger_ratio: startup_config.compact_trigger_ratio,
        keep_recent_turns: startup_config.keep_recent_turns,
        thinking: startup_config.thinking,
    };

    let provider = select_provider(&transient, credentials).map_err(|err| err.to_string())?;
    agent.set_provider(provider.clone());
    match kind {
        channel::ProviderSwitchKind::SessionRestore => agent.restore_model(model.to_string()),
        channel::ProviderSwitchKind::Interactive => agent.set_model(model.to_string(), history),
    }
    compacting.set_provider(provider);
    compacting.set_model(model.to_string());

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
    use super::channel::{
        AgentEvent, ChannelDecider, PermissionRequest, ProviderSwitchKind, UserInput,
    };
    use super::{
        activate_session_switch, apply_loaded_plan, apply_mouse_wheel_scroll_to_state,
        arrows_route_to_completion, cancel_action, complete_interrupted_tool_results,
        handle_mouse_selection_event, handle_queue_cancel_key, handle_resize, handle_selection_key,
        handle_session_picker_batch_key, initial_session_snapshot, isolate_network_approval_events,
        load_session_for_activation, normalize_loaded_session, prepare_session_startup,
        push_session_save_notice_if_error, run_agent_task, scroll_action_for_key,
        selection_key_action, send_session_provider_restore, should_exit, terminal_session_event,
        write_session_snapshot, CancelAction, ExitIntent, MouseWheelScrollAction,
        RunAgentTaskConfig, SelectionKeyAction, StartupMode, DEFAULT_SYSTEM_PROMPT,
        EXIT_DOUBLE_TAP,
    };
    use crate::agent::message::Message;
    use crate::agent::{Agent, AgentStatus, Compacting};
    use crate::app::assemble_agent;
    use crate::cli::CliPaths;
    use crate::config::{
        AuthType, Config, ProviderConfig, ProviderKind, ProviderProfile,
        DEFAULT_COMPACT_TRIGGER_RATIO, DEFAULT_KEEP_RECENT_TURNS, DEFAULT_THINKING,
    };
    use crate::error::ProviderError;
    use crate::permission::{PermissionMode, PermissionReply, PolicyEngine};
    use crate::provider::mock::MockProvider;
    use crate::provider::{
        DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, ThinkingBlock, ToolCall,
    };
    use crate::session::{SessionMeta, SessionStore, SessionSummary};
    use crate::tool::plan::StepStatus;
    use crate::tool::{
        run_blocking_tool, BlockingToolLimiter, PermissionLevel, Tool, ToolConcurrency,
        ToolContext, ToolOutcome, ToolRegistry,
    };
    use crate::tui::app::{ActivePlan, ActiveStep, CommandCompletion, TranscriptBlock};
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant as StdInstant;
    use tokio::sync::{mpsc, oneshot, Mutex};

    #[test]
    fn network_input_barrier_drops_queued_approval_events_only() {
        let retained = isolate_network_approval_events(vec![
            Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Event::Paste("y".to_string()),
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            Event::Resize(40, 10),
        ]);

        assert_eq!(retained.len(), 3);
        assert!(matches!(retained[0], Event::Key(key) if key.code == KeyCode::Down));
        assert!(matches!(retained[1], Event::Key(key) if key.code == KeyCode::Char('n')));
        assert!(matches!(retained[2], Event::Resize(40, 10)));
    }
    use tokio::time::{timeout, Duration};

    fn normal_channel_decider(tx: mpsc::UnboundedSender<AgentEvent>) -> ChannelDecider {
        ChannelDecider::new(
            tx,
            Arc::new(std::sync::Mutex::new(PermissionMode::Normal)),
            PolicyEngine::default(),
            PathBuf::from("user-config.toml"),
        )
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
            allowed_commands: Vec::new(),
            max_iterations: 4,
            timeout_secs: 30,
            model_context_window: None,
            compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
            thinking: DEFAULT_THINKING,
        }
    }

    fn cli_paths(temp: &tempfile::TempDir) -> CliPaths {
        CliPaths {
            user_config: temp.path().join("config.toml"),
            project_config: temp.path().join("mysteries.toml"),
            credentials: temp.path().join("credentials"),
            config_dir: temp.path().join("config"),
            cwd: temp.path().join("cwd"),
        }
    }

    fn session_meta(id: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_string(),
            provider: "alt".to_string(),
            model: "alt-model".to_string(),
            created_at: "123".to_string(),
            cwd: PathBuf::from("stored-cwd"),
            app_version: "1.1.0".to_string(),
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

    #[test]
    fn thinking_depth_injection_matches_config() {
        use crate::provider::Depth;

        let mut cfg = config();
        cfg.thinking = Depth::Medium;
        let thinking_depth = Arc::new(std::sync::Mutex::new(cfg.thinking));
        let mut assembled = assemble_agent(
            Arc::new(MockProvider::new(vec![ModelResponse {
                text: "ok".to_string(),
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            }])),
            &cfg,
            Box::new(normal_channel_decider(mpsc::unbounded_channel().0)),
            None,
            None,
            None,
        );
        assembled.agent.set_thinking_depth(thinking_depth.clone());
        assert_eq!(*thinking_depth.lock().unwrap(), Depth::Medium);
        *thinking_depth.lock().unwrap() = Depth::High;
        assert_eq!(*thinking_depth.lock().unwrap(), Depth::High);
    }

    #[test]
    fn startup_mode_prefers_resume_over_continue() {
        assert_eq!(super::startup_mode(true, true), StartupMode::Resume);
        assert_eq!(super::startup_mode(true, false), StartupMode::Resume);
        assert_eq!(super::startup_mode(false, true), StartupMode::Continue);
        assert_eq!(super::startup_mode(false, false), StartupMode::Fresh);
    }

    #[test]
    fn prepare_session_startup_continue_replaces_system_and_returns_provider_restore() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cli_paths(&temp);
        let store = SessionStore::new(paths.config_dir.join("sessions"));
        let meta = session_meta("resume-session");
        let history = vec![
            Message::System("old system".to_string()),
            Message::User("keep user".to_string()),
        ];
        let transcript = vec![TranscriptBlock::Notice("restored".to_string())];
        store.write(&meta, &history, &transcript, None).unwrap();

        let startup =
            prepare_session_startup(&store, &paths, &config(), StartupMode::Continue).unwrap();

        assert_eq!(startup.meta, meta);
        assert_eq!(
            startup.history,
            vec![
                Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                Message::User("keep user".to_string()),
            ]
        );
        assert_eq!(startup.transcript, transcript);
        assert_eq!(
            startup.resume_provider,
            Some(("alt".to_string(), "alt-model".to_string()))
        );
        assert_eq!(startup.plan, None);
    }

    #[test]
    fn prepare_session_startup_continue_returns_persisted_plan() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cli_paths(&temp);
        let store = SessionStore::new(paths.config_dir.join("sessions"));
        let meta = session_meta("resume-with-plan");
        let plan = ActivePlan {
            title: "持久化计划".to_string(),
            steps: vec![ActiveStep {
                description: "一步".to_string(),
                validation: "ok".to_string(),
                status: StepStatus::Done,
                validation_result: Some("passed".to_string()),
            }],
        };
        store
            .write(
                &meta,
                &[
                    Message::System("old system".to_string()),
                    Message::User("keep user".to_string()),
                ],
                &[TranscriptBlock::Notice("restored".to_string())],
                Some(&plan),
            )
            .unwrap();

        let startup =
            prepare_session_startup(&store, &paths, &config(), StartupMode::Continue).unwrap();

        assert_eq!(startup.plan, Some(plan));
    }

    #[test]
    fn continue_fallback_snapshot_uses_startup_provider_and_rewrites_selected_session() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp.path().join("sessions"));
        let startup_config = config();
        let selected_meta = SessionMeta {
            id: "selected-session".to_string(),
            provider: "missing-provider".to_string(),
            model: "missing-model".to_string(),
            created_at: "1".to_string(),
            cwd: temp.path().join("cwd"),
            app_version: "1.1.0".to_string(),
        };
        let session_startup = super::SessionStartup {
            meta: selected_meta.clone(),
            history: vec![Message::System(DEFAULT_SYSTEM_PROMPT.to_string())],
            transcript: Vec::new(),
            resume_provider: Some((selected_meta.provider.clone(), selected_meta.model.clone())),
            plan: None,
        };

        let snapshot = initial_session_snapshot(&startup_config, selected_meta.cwd.clone());
        assert_eq!(snapshot.provider, startup_config.provider.id);
        assert_eq!(snapshot.model, startup_config.model);

        let state = super::app::AppState::with_session_and_history(
            snapshot,
            Arc::new(Mutex::new(session_startup.history.clone())),
        );
        write_session_snapshot(&store, &selected_meta, &state, &session_startup.history)
            .expect("rewrite selected session with active fallback provider");
        let (rewritten, ..) = store.load("selected-session").expect("selected session");
        assert_eq!(rewritten.id, "selected-session");
        assert_eq!(rewritten.provider, startup_config.provider.id);
        assert_eq!(rewritten.model, startup_config.model);
    }

    #[test]
    fn apply_loaded_plan_sets_current_plan_from_some() {
        let mut state = crate::tui::app::AppState::new();
        let plan = ActivePlan {
            title: "还原计划".to_string(),
            steps: vec![ActiveStep {
                description: "步".to_string(),
                validation: "v".to_string(),
                status: StepStatus::Pending,
                validation_result: None,
            }],
        };

        apply_loaded_plan(&mut state, Some(plan.clone()));

        assert_eq!(state.current_plan, Some(plan));
    }

    #[test]
    fn apply_loaded_plan_none_leaves_current_plan_none() {
        let mut state = crate::tui::app::AppState::new();
        state.current_plan = Some(ActivePlan {
            title: "应被清空".to_string(),
            steps: vec![],
        });

        apply_loaded_plan(&mut state, None);

        assert!(state.current_plan.is_none());
    }

    #[test]
    fn prepare_session_startup_resume_starts_fresh_for_picker() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cli_paths(&temp);
        let store = SessionStore::new(paths.config_dir.join("sessions"));
        store
            .write(
                &session_meta("stored-session"),
                &[Message::System("old system".to_string())],
                &[TranscriptBlock::Notice("stored".to_string())],
                None,
            )
            .unwrap();

        let startup =
            prepare_session_startup(&store, &paths, &config(), StartupMode::Resume).unwrap();

        assert_ne!(startup.meta.id, "stored-session");
        assert_eq!(
            startup.history,
            vec![Message::System(DEFAULT_SYSTEM_PROMPT.to_string())]
        );
        assert!(startup.transcript.is_empty());
        assert_eq!(startup.resume_provider, None);
    }

    #[test]
    fn terminal_session_event_matches_only_persisted_turn_boundaries() {
        assert!(terminal_session_event(&AgentEvent::TurnComplete));
        assert!(terminal_session_event(&AgentEvent::CompactDone));
        assert!(terminal_session_event(&AgentEvent::Interrupted));
        assert!(terminal_session_event(&AgentEvent::Error(
            "boom".to_string()
        )));
        assert!(!terminal_session_event(&AgentEvent::TextDelta(
            "delta".to_string()
        )));
        assert!(!terminal_session_event(&AgentEvent::Notice(
            "notice".to_string()
        )));
    }

    #[test]
    fn session_save_failure_appends_notice_without_touching_history() {
        let temp = tempfile::tempdir().unwrap();
        let bad_root = temp.path().join("not-a-directory");
        fs::write(&bad_root, "file blocks create_dir_all").unwrap();
        let store = SessionStore::new(bad_root);
        let mut state = super::app::AppState::with_session(super::app::SessionSnapshot {
            provider: "current-provider".to_string(),
            model: "current-model".to_string(),
            max_iterations: 4,
            cwd: temp.path().to_path_buf(),
            tools: 7,
        });
        state
            .transcript
            .push(TranscriptBlock::User("before".to_string()));
        let history = vec![
            Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
            Message::User("kept".to_string()),
        ];
        let original_history = history.clone();

        let save_result =
            write_session_snapshot(&store, &session_meta("failed-save"), &state, &history);
        push_session_save_notice_if_error(&mut state, save_result);

        assert_eq!(history, original_history);
        assert_eq!(
            state.transcript.last(),
            Some(&TranscriptBlock::Notice("会话保存失败".to_string()))
        );
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
                thinking: Vec::new(),
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

        fn get_text(&mut self) -> Result<String, String> {
            Ok(String::new())
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
    fn exit_intent_action_narrow_gap_exits() {
        assert_eq!(
            super::exit_intent_action(std::time::Duration::from_millis(500), EXIT_DOUBLE_TAP,),
            ExitIntent::Exit
        );
    }

    #[test]
    fn exit_intent_action_wide_gap_is_consumed() {
        assert_eq!(
            super::exit_intent_action(std::time::Duration::from_millis(1500), EXIT_DOUBLE_TAP,),
            ExitIntent::Consumed
        );
    }

    #[test]
    fn exit_intent_action_boundary_gap_equals_threshold_is_consumed() {
        assert_eq!(
            super::exit_intent_action(EXIT_DOUBLE_TAP, EXIT_DOUBLE_TAP),
            ExitIntent::Consumed
        );
    }

    #[test]
    fn idle_exit_intent_first_ctrl_c_consumes_and_records_time() {
        let mut state = super::app::AppState::new();
        let now = StdInstant::now();

        assert_eq!(
            super::handle_idle_exit_intent_key(&mut state, ctrl_c(), now),
            Some(ExitIntent::Consumed)
        );
        assert_eq!(state.last_exit_intent_at(), Some(now));
    }

    #[test]
    fn idle_exit_intent_second_ctrl_c_within_threshold_exits() {
        let mut state = super::app::AppState::new();
        let first = StdInstant::now();
        state.set_last_exit_intent_at(first);

        assert_eq!(
            super::handle_idle_exit_intent_key(
                &mut state,
                ctrl_c(),
                first + std::time::Duration::from_millis(100),
            ),
            Some(ExitIntent::Exit)
        );
        assert_eq!(state.last_exit_intent_at(), Some(first));
    }

    #[test]
    fn idle_exit_intent_ctrl_c_after_timeout_rearms() {
        let mut state = super::app::AppState::new();
        let first = StdInstant::now();
        let second = first + EXIT_DOUBLE_TAP + std::time::Duration::from_millis(1);
        state.set_last_exit_intent_at(first);

        assert_eq!(
            super::handle_idle_exit_intent_key(&mut state, ctrl_c(), second),
            Some(ExitIntent::Consumed)
        );
        assert_eq!(state.last_exit_intent_at(), Some(second));
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
            permission_level: crate::tool::PermissionLevel::Edit,
            network_preview: None,
            allow_always_key: None,
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

    // --- fix-queue-cancel-modal-priority Task 1(RED):浮层活跃时取消排队让位 ---

    #[test]
    fn handle_queue_cancel_key_yields_to_models_picker() {
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("queued".to_string());
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
        state.models_picker = Some(super::app::ModelsPicker::new(
            &profiles,
            ("alt", "alt-model"),
        ));

        assert!(!handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            StdInstant::now(),
        ));
        assert!(interrupt_rx.try_recv().is_err());
        assert_eq!(state.last_cancel_at(), None);
        assert!(state.has_queue());
    }

    #[test]
    fn handle_queue_cancel_key_yields_to_command_completion() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.enqueue_prompt("queued".to_string());
        state.on_key(key(KeyCode::Char('/')), &input_tx);
        assert!(state.command_completion.is_some());

        assert!(!handle_queue_cancel_key(
            &mut state,
            key(KeyCode::Esc),
            &interrupt_tx,
            StdInstant::now(),
        ));
        assert!(interrupt_rx.try_recv().is_err());
        assert_eq!(state.last_cancel_at(), None);
        assert!(state.has_queue());
    }

    // --- fix-queue-cancel-modal-priority Task 2:推进闸门集成测试(add-message-queue 3.4 补课) ---

    fn feed_agent_event(
        state: &mut super::app::AppState,
        event: AgentEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        let mut calling_model_started_at = None;
        let mut first_token_at = None;
        super::handle_agent_event(
            state,
            event,
            &mut calling_model_started_at,
            &mut first_token_at,
            input_tx,
        );
    }

    #[test]
    fn turn_complete_advances_exactly_one_queued_prompt() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.phase = super::app::Phase::CallingModel;
        state.enqueue_prompt("B".to_string());
        state.enqueue_prompt("C".to_string());

        feed_agent_event(&mut state, AgentEvent::TurnComplete, &input_tx);

        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("B".to_string())
        );
        assert!(
            input_rx.try_recv().is_err(),
            "channel must hold at most one advanced prompt"
        );
        assert_eq!(state.pending_queue, vec!["C".to_string()]);
        assert_eq!(state.phase, super::app::Phase::Busy);
        assert_eq!(
            state.transcript.last(),
            Some(&super::app::TranscriptBlock::User("B".to_string()))
        );
    }

    #[test]
    fn compact_done_advances_exactly_one_queued_prompt() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.phase = super::app::Phase::Compacting;
        state.enqueue_prompt("B".to_string());
        state.enqueue_prompt("C".to_string());

        feed_agent_event(&mut state, AgentEvent::CompactDone, &input_tx);

        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("B".to_string())
        );
        assert!(
            input_rx.try_recv().is_err(),
            "channel must hold at most one advanced prompt"
        );
        assert_eq!(state.pending_queue, vec!["C".to_string()]);
        assert_eq!(state.phase, super::app::Phase::Busy);
    }

    #[test]
    fn error_terminal_event_still_advances_queue() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.phase = super::app::Phase::CallingModel;
        state.enqueue_prompt("B".to_string());

        feed_agent_event(&mut state, AgentEvent::Error("boom".to_string()), &input_tx);

        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("B".to_string())
        );
        assert!(input_rx.try_recv().is_err());
        assert!(!state.has_queue());
        assert_eq!(state.phase, super::app::Phase::Busy);
    }

    #[test]
    fn interrupted_terminal_event_still_advances_queue() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.phase = super::app::Phase::CallingModel;
        state.enqueue_prompt("B".to_string());

        feed_agent_event(&mut state, AgentEvent::Interrupted, &input_tx);

        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("B".to_string())
        );
        assert!(input_rx.try_recv().is_err());
        assert!(!state.has_queue());
        assert_eq!(state.phase, super::app::Phase::Busy);
    }

    #[test]
    fn idle_does_not_advance_and_idle_window_submit_enqueues() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.phase = super::app::Phase::CallingModel;
        state.enqueue_prompt("B".to_string());

        feed_agent_event(
            &mut state,
            AgentEvent::StatusChanged(AgentStatus::Idle),
            &input_tx,
        );
        assert!(input_rx.try_recv().is_err(), "Idle must not advance queue");
        assert_eq!(state.phase, super::app::Phase::CallingModel);
        assert_eq!(state.pending_queue, vec!["B".to_string()]);

        // Idle→TurnComplete 窗口内提交:入队、不直发
        state.set_input_text("X");
        state.on_key(key(KeyCode::Enter), &input_tx);
        assert!(
            input_rx.try_recv().is_err(),
            "submit in Idle window must enqueue, not direct-send"
        );
        assert_eq!(state.pending_queue, vec!["B".to_string(), "X".to_string()]);

        // 随后 TurnComplete:恰推进一条(队首 B),无 double-send
        feed_agent_event(&mut state, AgentEvent::TurnComplete, &input_tx);
        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("B".to_string())
        );
        assert!(input_rx.try_recv().is_err());
        assert_eq!(state.pending_queue, vec!["X".to_string()]);
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
            permission_level: crate::tool::PermissionLevel::Edit,
            network_preview: None,
            allow_always_key: None,
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
            permission_level: crate::tool::PermissionLevel::Edit,
            network_preview: None,
            allow_always_key: None,
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
        assert!(!should_exit(&ready, ctrl_c()));
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
                thinking: Vec::new(),
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            },
        ]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider,
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
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
                request.responder.send(PermissionReply::AllowOnce).unwrap();
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
                thinking: Vec::new(),
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            },
        ]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
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
                request.responder.send(PermissionReply::AllowOnce).unwrap();
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
            thinking: Vec::new(),
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
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
                thinking: Vec::new(),
            },
            ModelResponse {
                text: "second reply".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
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
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            assembled.compacting,
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
                    Message::Assistant { text, tool_calls, .. }
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
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
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
        let trailing = timeout(Duration::from_millis(80), ui_rx.recv()).await;
        assert!(
            trailing.is_err(),
            "Interrupted must not be followed by any trailing event (esp. StatusChanged(Idle)), got {trailing:?}"
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
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
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
            thinking: Vec::new(),
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
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, profiles);
        let history = agent_history();
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "alt".to_string(),
                model: "alt-model".to_string(),
                kind: ProviderSwitchKind::Interactive,
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
            thinking: Vec::new(),
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "missing".to_string(),
                model: "m1".to_string(),
                kind: ProviderSwitchKind::Interactive,
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
            thinking: Vec::new(),
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
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, profiles);
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "wps".to_string(),
                model: "zhipu/glm-5.2".to_string(),
                kind: ProviderSwitchKind::Interactive,
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
            thinking: Vec::new(),
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            old_provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
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
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx
            .send(UserInput::SetProvider {
                id: "wps".to_string(),
                model: "zhipu/glm-5.2".to_string(),
                kind: ProviderSwitchKind::Interactive,
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

    // --- §6.2 RED: interrupt history + loaded session normalization ---

    const INTERRUPTED_TOOL_CONTENT: &str = "tool call interrupted before completion";
    const PREV_SESSION_INTERRUPTED_OUTPUT: &str = "上次会话已中断";

    #[test]
    fn complete_interrupted_tool_results_fills_unpaired_occurrences() {
        use crate::provider::ToolCall;
        use serde_json::json;

        let mut history = vec![
            Message::System("sys".into()),
            Message::User("old turn".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "old-1".into(),
                    name: "echo".into(),
                    arguments: json!({}),
                }],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "old-1".into(),
                content: "old ok".into(),
                is_error: false,
            },
            // turn start
            Message::User("new turn".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "c1".into(),
                        name: "safe".into(),
                        arguments: json!({"key": "a"}),
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "safe".into(),
                        arguments: json!({"key": "b"}),
                    },
                    ToolCall {
                        id: "c3".into(),
                        name: "safe".into(),
                        arguments: json!({"key": "c"}),
                    },
                ],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "c1".into(),
                content: "ok:a".into(),
                is_error: false,
            },
        ];
        let turn_start = 4; // index of User("new turn")
        complete_interrupted_tool_results(&mut history, turn_start);

        let results: Vec<_> = history[turn_start..]
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => Some((call_id.as_str(), content.as_str(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("c1", "ok:a", false),
                ("c2", INTERRUPTED_TOOL_CONTENT, true),
                ("c3", INTERRUPTED_TOOL_CONTENT, true),
            ]
        );
        // 更早 turn 不变
        assert!(matches!(
            &history[3],
            Message::ToolResult {
                call_id,
                content,
                is_error: false
            } if call_id == "old-1" && content == "old ok"
        ));
    }

    #[test]
    fn complete_interrupted_respects_duplicate_ids_across_assistants() {
        use crate::provider::ToolCall;
        use serde_json::json;

        let mut history = vec![
            Message::User("turn".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "safe".into(),
                    arguments: json!({"key": "a"}),
                }],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "call-1".into(),
                content: "ok:a".into(),
                is_error: false,
            },
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".into(),
                    name: "safe".into(),
                    arguments: json!({"key": "b"}),
                }],
                thinking: Vec::new(),
            },
        ];
        complete_interrupted_tool_results(&mut history, 0);
        let results: Vec<_> = history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => Some((call_id.as_str(), content.as_str(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            vec![
                ("call-1", "ok:a", false),
                ("call-1", INTERRUPTED_TOOL_CONTENT, true),
            ]
        );
    }

    #[test]
    fn normalize_loaded_session_closes_running_cards_and_fills_history() {
        use crate::provider::ToolCall;
        use crate::tui::app::{ToolCard, ToolCardStatus};
        use serde_json::json;

        let mut history = vec![
            Message::System("sys".into()),
            Message::User("u".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                ],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "call-1".into(),
                content: "first".into(),
                is_error: false,
            },
            Message::User("later".into()),
        ];
        let mut transcript = vec![
            TranscriptBlock::User("u".into()),
            TranscriptBlock::Tool(ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: ToolCardStatus::Done,
                output: Some("first".into()),
                truncated: false,
                exit: None,
            }),
            TranscriptBlock::Tool(ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: ToolCardStatus::Running,
                output: None,
                truncated: true,
                exit: Some(1),
            }),
            TranscriptBlock::Notice("keep".into()),
            TranscriptBlock::Tool(ToolCard {
                id: "other".into(),
                name: "grep".into(),
                args: json!({}),
                readonly: true,
                status: ToolCardStatus::Error,
                output: Some("err".into()),
                truncated: false,
                exit: None,
            }),
        ];

        normalize_loaded_session(&mut history, &mut transcript);
        // 第二 occurrence 应在结果组末尾、later User 之前
        assert!(matches!(
            &history[4],
            Message::ToolResult {
                call_id,
                content,
                is_error: true
            } if call_id == "call-1" && content == INTERRUPTED_TOOL_CONTENT
        ));
        assert!(matches!(&history[5], Message::User(t) if t == "later"));

        match &transcript[2] {
            TranscriptBlock::Tool(card) => {
                assert_eq!(card.status, ToolCardStatus::Error);
                assert_eq!(
                    card.output.as_deref(),
                    Some(PREV_SESSION_INTERRUPTED_OUTPUT)
                );
                assert!(!card.truncated);
                assert_eq!(card.exit, None);
            }
            other => panic!("expected tool card, got {other:?}"),
        }
        // Done / Notice / Error 不变
        match &transcript[1] {
            TranscriptBlock::Tool(card) => {
                assert_eq!(card.status, ToolCardStatus::Done);
                assert_eq!(card.output.as_deref(), Some("first"));
            }
            _ => panic!("done card"),
        }
        assert!(matches!(&transcript[3], TranscriptBlock::Notice(t) if t == "keep"));

        // 幂等
        let history_once = history.clone();
        let transcript_once = transcript.clone();
        normalize_loaded_session(&mut history, &mut transcript);
        assert_eq!(history, history_once);
        assert_eq!(transcript, transcript_once);
    }

    #[test]
    fn load_session_for_activation_normalizes_while_raw_load_unchanged() {
        use crate::provider::ToolCall;
        use crate::tui::app::{ToolCard, ToolCardStatus};
        use serde_json::json;

        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp.path().to_path_buf());
        let meta = SessionMeta {
            id: "sess-norm".into(),
            provider: "mock".into(),
            model: "m".into(),
            created_at: "1".into(),
            cwd: temp.path().to_path_buf(),
            app_version: "1".into(),
        };
        let history = vec![
            Message::System("old sys".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                ],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "call-1".into(),
                content: "first".into(),
                is_error: false,
            },
        ];
        let transcript = vec![TranscriptBlock::Tool(ToolCard {
            id: "call-1".into(),
            name: "read_file".into(),
            args: json!({}),
            readonly: true,
            status: ToolCardStatus::Running,
            output: None,
            truncated: false,
            exit: None,
        })];
        store
            .write(&meta, &history, &transcript, None)
            .expect("write");

        let raw = store.load("sess-norm").expect("raw load");
        assert_eq!(raw.1, history);
        assert!(matches!(
            &raw.2[0],
            TranscriptBlock::Tool(card) if card.status == ToolCardStatus::Running
        ));

        let activated = load_session_for_activation(&store, "sess-norm").expect("activate");
        assert!(
            activated.1.iter().any(|m| matches!(
                m,
                Message::ToolResult {
                    call_id,
                    content,
                    is_error: true
                } if call_id == "call-1" && content == INTERRUPTED_TOOL_CONTENT
            )),
            "activation must fill second occurrence interrupted result"
        );
        assert!(matches!(
            &activated.2[0],
            TranscriptBlock::Tool(card)
                if card.status == ToolCardStatus::Error
                    && card.output.as_deref() == Some(PREV_SESSION_INTERRUPTED_OUTPUT)
        ));
        // 磁盘未改
        let raw_again = store.load("sess-norm").expect("raw again");
        assert_eq!(raw_again.1, history);
    }

    // --- provider 事务 / thinking 保留 / 双工具 Interrupt / session→Provider 回归 ---

    use std::sync::Mutex as StdMutex;

    /// 审查 #3：恢复路径 `apply_set_provider` → `set_model` 不得清空 loaded thinking。
    #[test]
    fn apply_set_provider_restore_preserves_assistant_thinking() {
        let temp = tempfile::tempdir().unwrap();
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "mock".to_string(),
            ProviderProfile {
                id: "mock".to_string(),
                kind: ProviderKind::Mock,
                base_url: None,
                model: "restored-model".to_string(),
                auth_type: AuthType::ApiKey,
            },
        );
        let provider = Arc::new(MockProvider::new(vec![]));
        let mut assembled = assemble_agent(
            provider,
            &config(),
            Box::new(normal_channel_decider(mpsc::unbounded_channel().0)),
            None,
            None,
            None,
        );
        let thinking = vec![ThinkingBlock {
            text: "keep-me".to_string(),
            signature: Some("sig-restore".to_string()),
            redacted: false,
        }];
        let mut history = vec![
            Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
            Message::User("hi".into()),
            Message::Assistant {
                text: "prior".into(),
                tool_calls: Vec::new(),
                thinking: thinking.clone(),
            },
        ];

        super::apply_set_provider(
            &profiles,
            &config(),
            &temp.path().join("credentials"),
            "mock",
            "restored-model",
            &mut assembled.agent,
            &mut assembled.compacting,
            &mut history,
            ProviderSwitchKind::SessionRestore,
        )
        .expect("mock provider restore should succeed");

        match &history[2] {
            Message::Assistant {
                thinking: blocks, ..
            } => assert_eq!(
                blocks, &thinking,
                "审查#3：session 恢复成功时必须保留 Assistant.thinking"
            ),
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    /// 审查 #2：unknown provider — Agent 保持 startup、仅一次 Notice、无 ProviderApplied。
    #[tokio::test]
    async fn set_provider_unknown_keeps_agent_and_single_notice_no_ui_commit() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "still-startup".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }]));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, BTreeMap::new());
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        let mut state = super::app::AppState::new();
        state.session.provider = "mock".to_string();
        state.session.model = "tui-test-model".to_string();

        input_tx
            .send(UserInput::SetProvider {
                id: "ghost-provider".to_string(),
                model: "ghost-model".to_string(),
                kind: ProviderSwitchKind::SessionRestore,
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("ping".to_string()))
            .unwrap();

        let mut notices = Vec::new();
        let mut applied = 0usize;
        loop {
            match ui_rx.recv().await.expect("ui") {
                AgentEvent::Notice(t) => {
                    notices.push(t.clone());
                    state.apply(AgentEvent::Notice(t));
                }
                AgentEvent::ProviderApplied { id, model } => {
                    applied += 1;
                    state.apply(AgentEvent::ProviderApplied { id, model });
                }
                AgentEvent::TurnComplete => break,
                other => state.apply(other),
            }
        }
        drop(input_tx);
        handle.await.unwrap();

        assert_eq!(
            notices.len(),
            1,
            "审查#2：失败只产生一次 Notice, got {notices:?}"
        );
        assert!(
            notices[0].contains("未知 provider") || notices[0].contains("ghost"),
            "notice={:?}",
            notices[0]
        );
        assert_eq!(applied, 0, "失败不得发 ProviderApplied");
        assert_eq!(provider.recorded_requests().len(), 1);
        assert_eq!(provider.recorded_requests()[0].model, "tui-test-model");
        assert_eq!(
            state.session.provider, "mock",
            "审查#2：unknown provider 失败后 UI 不得保留无效 provider"
        );
        assert_eq!(state.session.model, "tui-test-model");
    }

    /// 审查 #2：缺凭据 — Agent 保持 startup；UI 不得停在无效 provider。
    #[tokio::test]
    async fn set_provider_missing_creds_keeps_ui_on_startup_provider() {
        let temp = tempfile::tempdir().unwrap();
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "still-startup".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
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
                base_url: Some("https://example.invalid/v1".to_string()),
                model: "remote-model".to_string(),
                auth_type: AuthType::ApiKey,
            },
        );
        let assembled = assemble_agent(
            provider.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, profiles);
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            agent_history(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        let mut state = super::app::AppState::new();
        state.session.provider = "mock".to_string();
        state.session.model = "tui-test-model".to_string();

        input_tx
            .send(UserInput::SetProvider {
                id: "wps".to_string(),
                model: "remote-model".to_string(),
                kind: ProviderSwitchKind::SessionRestore,
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("ping".to_string()))
            .unwrap();

        let mut notices = 0usize;
        let mut applied = 0usize;
        loop {
            match ui_rx.recv().await.expect("ui") {
                AgentEvent::Notice(t) if t.contains("凭据") => {
                    notices += 1;
                    state.apply(AgentEvent::Notice(t));
                }
                AgentEvent::ProviderApplied { id, model } => {
                    applied += 1;
                    state.apply(AgentEvent::ProviderApplied { id, model });
                }
                AgentEvent::TurnComplete => break,
                other => state.apply(other),
            }
        }
        drop(input_tx);
        handle.await.unwrap();

        assert_eq!(notices, 1);
        assert_eq!(applied, 0);
        assert_eq!(provider.recorded_requests().len(), 1);
        assert_eq!(
            state.session.provider, "mock",
            "审查#2：缺凭据失败后 UI 保持 startup provider"
        );
        assert_eq!(state.session.model, "tui-test-model");
    }

    /// 审查 #2：成功恢复 — 仅 ProviderApplied 后 UI 提交，与 Agent 一致。
    #[tokio::test]
    async fn set_provider_success_allows_ui_commit_to_restored_provider() {
        let temp = tempfile::tempdir().unwrap();
        let old = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "old".into(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
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
            old.clone(),
            &config(),
            Box::new(normal_channel_decider(ui_tx.clone())),
            None,
            None,
            None,
        );
        let task_config = task_hotswap(&temp, profiles);
        let history = agent_history();
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        let mut state = super::app::AppState::new();
        state.session.provider = "mock".to_string();
        state.session.model = "tui-test-model".to_string();

        input_tx
            .send(UserInput::SetProvider {
                id: "alt".to_string(),
                model: "alt-model".to_string(),
                kind: ProviderSwitchKind::SessionRestore,
            })
            .unwrap();
        input_tx
            .send(UserInput::Prompt("hello".to_string()))
            .unwrap();
        loop {
            match ui_rx.recv().await.expect("ui") {
                AgentEvent::ProviderApplied { id, model } => {
                    state.apply(AgentEvent::ProviderApplied { id, model });
                }
                AgentEvent::TurnComplete => break,
                other => state.apply(other),
            }
        }
        drop(input_tx);
        handle.await.unwrap();

        assert!(old.recorded_requests().is_empty());
        assert_eq!(state.session.provider, "alt");
        assert_eq!(state.session.model, "alt-model");
        let locked = history.lock().await;
        assert!(locked
            .iter()
            .any(|m| matches!(m, Message::User(t) if t == "hello")));
    }

    /// 审查 #2 生产路径：models picker Enter 当前乐观写 session.provider；
    /// 事务性修复后，在 Agent 确认前不得改 UI（与 session activation 同构）。
    #[test]
    fn models_picker_enter_must_not_commit_provider_before_agent_confirms() {
        use crate::tui::command::Command;

        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = super::app::AppState::new();
        state.session.provider = "mock".to_string();
        state.session.model = "tui-test-model".to_string();
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "mock".to_string(),
            ProviderProfile {
                id: "mock".to_string(),
                kind: ProviderKind::Mock,
                base_url: None,
                model: "tui-test-model".to_string(),
                auth_type: AuthType::ApiKey,
            },
        );
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
        state.provider_profiles = profiles;
        state.execute_command(Command::Models, &input_tx);
        // 移到 alt
        if let Some(picker) = state.models_picker.as_mut() {
            for _ in 0..8 {
                if picker.selected().map(|(id, _)| id) == Some("alt".to_string()) {
                    break;
                }
                picker.move_highlight(1);
            }
        }
        state.on_key(key(KeyCode::Enter), &input_tx);

        match input_rx.try_recv() {
            Ok(UserInput::SetProvider { id, model, .. }) => {
                assert_eq!(id, "alt");
                assert_eq!(model, "alt-model");
            }
            other => panic!("expected SetProvider, got {other:?}"),
        }
        // Agent 尚未确认前，UI 必须仍保留当前 provider/model。
        assert_eq!(
            state.session.provider, "mock",
            "审查#2：不得在 Agent 确认前乐观写入 provider"
        );
        assert_eq!(state.session.model, "tui-test-model");
    }

    /// 审查 #6.1–6.2：双 ParallelSafe 工具均 entered 后 Interrupt。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn run_agent_task_interrupts_two_parallel_tools_without_late_events() {
        let temp = tempfile::tempdir().unwrap();
        let limiter = BlockingToolLimiter::new(4);
        let (e1_tx, mut e1_rx) = oneshot::channel::<()>();
        let (e2_tx, mut e2_rx) = oneshot::channel::<()>();
        let (r1_tx, r1_rx) = std::sync::mpsc::channel::<()>();
        let (r2_tx, r2_rx) = std::sync::mpsc::channel::<()>();
        let (done1_tx, mut done1_rx) = oneshot::channel::<()>();
        let (done2_tx, mut done2_rx) = oneshot::channel::<()>();
        let (watchdog_cancel_tx, watchdog_cancel_rx) = std::sync::mpsc::channel::<()>();
        let (watchdog_failed_tx, mut watchdog_failed_rx) = mpsc::unbounded_channel::<()>();
        let watchdog = {
            let r1 = r1_tx.clone();
            let r2 = r2_tx.clone();
            std::thread::spawn(move || {
                if watchdog_cancel_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .is_err()
                {
                    let _ = r1.send(());
                    let _ = r2.send(());
                    let _ = watchdog_failed_tx.send(());
                }
            })
        };

        struct BlockingHoldTool {
            name: &'static str,
            limiter: BlockingToolLimiter,
            entered: StdMutex<Option<oneshot::Sender<()>>>,
            release: StdMutex<Option<std::sync::mpsc::Receiver<()>>>,
            completed: StdMutex<Option<oneshot::Sender<()>>>,
        }

        #[async_trait]
        impl Tool for BlockingHoldTool {
            fn name(&self) -> &str {
                self.name
            }
            fn description(&self) -> &str {
                "hold"
            }
            fn schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {}})
            }
            fn permission_level(&self) -> PermissionLevel {
                PermissionLevel::ReadOnly
            }
            fn concurrency(&self) -> ToolConcurrency {
                ToolConcurrency::ParallelSafe
            }
            async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolOutcome {
                let entered = self.entered.lock().unwrap().take().expect("entered once");
                let release = self
                    .release
                    .lock()
                    .unwrap()
                    .take()
                    .expect("release rx once");
                let completed = self
                    .completed
                    .lock()
                    .unwrap()
                    .take()
                    .expect("completed once");
                let name = self.name;
                run_blocking_tool(&self.limiter, move || {
                    let _ = entered.send(());
                    let _ = release.recv();
                    let _ = completed.send(());
                    ToolOutcome {
                        content: format!("done:{name}"),
                        is_error: false,
                        truncated: false,
                        exit: None,
                    }
                })
                .await
            }
        }

        let mut registry = ToolRegistry::new();
        registry
            .register(Box::new(BlockingHoldTool {
                name: "hold_a",
                limiter: limiter.clone(),
                entered: StdMutex::new(Some(e1_tx)),
                release: StdMutex::new(Some(r1_rx)),
                completed: StdMutex::new(Some(done1_tx)),
            }))
            .unwrap();
        registry
            .register(Box::new(BlockingHoldTool {
                name: "hold_b",
                limiter,
                entered: StdMutex::new(Some(e2_tx)),
                release: StdMutex::new(Some(r2_rx)),
                completed: StdMutex::new(Some(done2_tx)),
            }))
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            ModelResponse {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "c1".into(),
                        name: "hold_a".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "hold_b".into(),
                        arguments: json!({}),
                    },
                ],
                finish_reason: FinishReason::ToolCalls,
                usage: None,
                thinking: Vec::new(),
            },
            // 中断后下一 Prompt 使用
            ModelResponse {
                text: "next-ok".into(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            },
        ]));

        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let history = agent_history();
        let mut agent = Agent::new(
            provider,
            registry,
            Box::new(normal_channel_decider(ui_tx.clone())),
            "tui-test-model".to_string(),
            4,
        );
        agent.set_permission_mode(Arc::new(std::sync::Mutex::new(PermissionMode::Normal)));
        let compacting = Compacting::new(
            Arc::new(MockProvider::new(vec![])),
            "tui-test-model".to_string(),
            crate::agent::CompactionSettings {
                model_context_window: None,
                compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
                keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
            },
        );
        let handle = tokio::spawn(run_agent_task(
            agent,
            history.clone(),
            compacting,
            task_hotswap(&temp, BTreeMap::new()),
            input_rx,
            interrupt_rx,
            ui_tx,
        ));

        input_tx.send(UserInput::Prompt("run both".into())).unwrap();

        // 等待两个真实 spawn_blocking closure 均 entered；OS watchdog 仅负责失败清理。
        tokio::select! {
            result = &mut e1_rx => result.expect("hold_a entered"),
            _ = watchdog_failed_rx.recv() => panic!("watchdog fired before hold_a entered"),
        }
        tokio::select! {
            result = &mut e2_rx => result.expect("hold_b entered"),
            _ = watchdog_failed_rx.recv() => panic!("watchdog fired before hold_b entered"),
        }

        interrupt_tx.send(UserInput::Interrupt).unwrap();

        let mut interrupted = 0usize;
        let mut finished = 0usize;
        let mut idle = 0usize;
        loop {
            tokio::select! {
                event = ui_rx.recv() => match event.expect("ui") {
                    AgentEvent::Interrupted => {
                        interrupted += 1;
                        break;
                    }
                    AgentEvent::ToolCallFinished { .. } => finished += 1,
                    AgentEvent::StatusChanged(AgentStatus::Idle) => idle += 1,
                    _ => {}
                },
                _ = watchdog_failed_rx.recv() => {
                    panic!("watchdog fired before Interrupted");
                }
            }
        }
        assert_eq!(interrupted, 1, "exactly one Interrupted");
        assert_eq!(finished, 0, "no ToolCallFinished after interrupt path");
        assert_eq!(idle, 0, "no Idle after interrupt");
        assert!(matches!(
            ui_rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert!(!handle.is_finished(), "Agent task 仍存活");

        // history：每个 occurrence 恰有一个 interrupted ToolResult
        {
            let h = history.lock().await;
            let results: Vec<_> = h
                .iter()
                .filter_map(|m| match m {
                    Message::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } => Some((call_id.as_str(), content.as_str(), *is_error)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                results,
                vec![
                    ("c1", INTERRUPTED_TOOL_CONTENT, true),
                    ("c2", INTERRUPTED_TOOL_CONTENT, true),
                ]
            );
        }

        // 释放 detached blocking closures，并以 completed ack 代替固定 sleep。
        let _ = r1_tx.send(());
        let _ = r2_tx.send(());
        tokio::select! {
            result = &mut done1_rx => result.expect("hold_a completed"),
            _ = watchdog_failed_rx.recv() => panic!("watchdog fired before hold_a completed"),
        }
        tokio::select! {
            result = &mut done2_rx => result.expect("hold_b completed"),
            _ = watchdog_failed_rx.recv() => panic!("watchdog fired before hold_b completed"),
        }
        let _ = watchdog_cancel_tx.send(());
        let _ = watchdog.join();
        tokio::task::yield_now().await;
        assert!(
            matches!(ui_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "detached blocking outcomes must not emit late events"
        );
        {
            let h = history.lock().await;
            let results: Vec<_> = h
                .iter()
                .filter_map(|m| match m {
                    Message::ToolResult {
                        call_id, content, ..
                    } => Some((call_id.as_str(), content.as_str())),
                    _ => None,
                })
                .collect();
            assert_eq!(
                results,
                vec![
                    ("c1", INTERRUPTED_TOOL_CONTENT),
                    ("c2", INTERRUPTED_TOOL_CONTENT),
                ],
                "释放后台 closure 后 history 不被污染"
            );
        }

        // 立即下一 Prompt 可正常完成
        input_tx.send(UserInput::Prompt("after".into())).unwrap();
        loop {
            match ui_rx.recv().await.expect("next prompt event") {
                AgentEvent::TurnComplete => break,
                AgentEvent::ToolCallFinished { .. } => {
                    panic!("detached prior tool emitted finished during next prompt")
                }
                _ => {}
            }
        }

        drop(input_tx);
        drop(interrupt_tx);
        handle.abort();
        let _ = handle.await;
    }

    /// 审查 #6.3–6.5：`--continue` / picker 激活 fixture → 首次 Provider 请求形状。
    #[tokio::test]
    async fn continue_activation_first_provider_sees_normalized_history_with_thinking() {
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp.path().join("sessions"));
        let meta = SessionMeta {
            id: "sess-continue".into(),
            provider: "mock".into(),
            model: "restored-model".into(),
            created_at: "1".into(),
            cwd: temp.path().to_path_buf(),
            app_version: "1".into(),
        };
        let thinking = vec![ThinkingBlock {
            text: "session-thought".into(),
            signature: Some("sig-s".into()),
            redacted: false,
        }];
        let history = vec![
            Message::System("old-system-should-replace".into()),
            Message::User("u".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                ],
                thinking: thinking.clone(),
            },
            Message::ToolResult {
                call_id: "call-1".into(),
                content: "first-only".into(),
                is_error: false,
            },
        ];
        let transcript = vec![
            TranscriptBlock::User("u".into()),
            TranscriptBlock::Tool(super::app::ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: super::app::ToolCardStatus::Done,
                output: Some("first-only".into()),
                truncated: false,
                exit: None,
            }),
            TranscriptBlock::Tool(super::app::ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: super::app::ToolCardStatus::Running,
                output: None,
                truncated: false,
                exit: None,
            }),
            TranscriptBlock::Tool(super::app::ToolCard {
                id: "other".into(),
                name: "grep".into(),
                args: json!({}),
                readonly: true,
                status: super::app::ToolCardStatus::Error,
                output: Some("err".into()),
                truncated: false,
                exit: None,
            }),
        ];
        store.write(&meta, &history, &transcript, None).unwrap();

        let startup =
            prepare_session_startup(&store, &cli_paths(&temp), &config(), StartupMode::Continue)
                .expect("continue startup");
        assert_eq!(startup.meta.id, "sess-continue");
        let loaded_history = startup.history;
        let loaded_transcript = startup.transcript;
        let restore_provider = startup.resume_provider.expect("restore provider");

        // 历史 Error/Done 卡：Running 已收口
        assert!(matches!(
            &loaded_transcript[1],
            TranscriptBlock::Tool(c) if c.status == super::app::ToolCardStatus::Done
        ));
        assert!(matches!(
            &loaded_transcript[2],
            TranscriptBlock::Tool(c)
                if c.status == super::app::ToolCardStatus::Error
                    && c.output.as_deref() == Some(PREV_SESSION_INTERRUPTED_OUTPUT)
        ));
        assert!(matches!(
            &loaded_transcript[3],
            TranscriptBlock::Tool(c) if c.status == super::app::ToolCardStatus::Error
        ));

        let recording = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "new-turn".into(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }]));
        let assembled = assemble_agent(
            recording.clone(),
            &config(),
            Box::new(normal_channel_decider(mpsc::unbounded_channel().0)),
            None,
            None,
            None,
        );
        // thinking 必须仍在 history（审查#3 + #6.4）
        match loaded_history.iter().find(|m| {
            matches!(
                m,
                Message::Assistant {
                    thinking: t,
                    ..
                } if !t.is_empty()
            )
        }) {
            Some(Message::Assistant {
                thinking: blocks, ..
            }) => assert_eq!(blocks, &thinking, "thinking must survive restore"),
            _ => panic!("thinking stripped during restore"),
        }

        // 每个 occurrence 均已配对
        let tool_results: Vec<_> = loaded_history
            .iter()
            .filter_map(|m| match m {
                Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tool_results, vec!["call-1", "call-1"]);

        let history = Arc::new(Mutex::new(loaded_history));
        let (restore_tx, mut restore_rx) = mpsc::unbounded_channel();
        send_session_provider_restore(&restore_tx, restore_provider.0, restore_provider.1);
        let restore_input = restore_rx.try_recv().expect("continue restore command");
        assert!(matches!(
            &restore_input,
            UserInput::SetProvider {
                kind: ProviderSwitchKind::SessionRestore,
                ..
            }
        ));
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_interrupt_tx, interrupt_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let task_config = RunAgentTaskConfig {
            // 故意不注册 persisted provider：真实 SetProvider 失败后回退 RecordingProvider。
            profiles: BTreeMap::new(),
            startup_config: config(),
            credentials_path: temp.path().join("credentials"),
            tool_ctx: ToolContext {
                cwd: temp.path().to_path_buf(),
                max_output_bytes: 4096,
            },
        };
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            history.clone(),
            assembled.compacting,
            task_config,
            input_rx,
            interrupt_rx,
            ui_tx,
        ));
        input_tx.send(restore_input).unwrap();
        input_tx.send(UserInput::Prompt("next".into())).unwrap();
        let mut fallback_notices = 0usize;
        loop {
            match ui_rx.recv().await {
                Some(AgentEvent::Notice(message)) if message.contains("未知 provider") => {
                    fallback_notices += 1;
                }
                Some(AgentEvent::TurnComplete) => break,
                Some(_) => {}
                None => panic!("ui channel closed before TurnComplete"),
            }
        }
        drop(input_tx);
        handle.await.unwrap();
        assert_eq!(fallback_notices, 1, "continue fallback emits one Notice");

        let recorded = recording.recorded_requests();
        assert_eq!(recorded.len(), 1, "first prompt after restore");
        let msgs = &recorded[0].messages;
        assert!(
            matches!(&msgs[0], Message::System(s) if s == DEFAULT_SYSTEM_PROMPT),
            "System 已替换"
        );
        assert!(
            msgs.iter().any(|m| matches!(
                m,
                Message::Assistant { thinking: t, .. } if t == &thinking
            )),
            "thinking 保留于首次 Provider 请求"
        );
        let call_count = msgs
            .iter()
            .filter(|m| matches!(m, Message::ToolResult { call_id, .. } if call_id == "call-1"))
            .count();
        assert_eq!(call_count, 2, "每个 occurrence 均配对，无 dangling");
    }

    /// 审查 #6.3 picker hot-swap 路径：与 continue 共用 load_session_for_activation。
    #[tokio::test]
    async fn picker_hotswap_activation_first_provider_matches_continue_contract() {
        // 与 continue 相同 fixture 契约；激活 seam 已是 load_session_for_activation。
        let temp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(temp.path().join("sessions"));
        let meta = SessionMeta {
            id: "sess-picker".into(),
            provider: "mock".into(),
            model: "restored-model".into(),
            created_at: "1".into(),
            cwd: temp.path().to_path_buf(),
            app_version: "1".into(),
        };
        let thinking = vec![ThinkingBlock {
            text: "picker-thought".into(),
            signature: None,
            redacted: false,
        }];
        let history = vec![
            Message::System("old".into()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "dup".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                    ToolCall {
                        id: "dup".into(),
                        name: "read_file".into(),
                        arguments: json!({}),
                    },
                ],
                thinking: thinking.clone(),
            },
            Message::ToolResult {
                call_id: "dup".into(),
                content: "one".into(),
                is_error: false,
            },
        ];
        let transcript = vec![
            TranscriptBlock::Tool(super::app::ToolCard {
                id: "dup".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: super::app::ToolCardStatus::Running,
                output: None,
                truncated: false,
                exit: None,
            }),
            TranscriptBlock::Tool(super::app::ToolCard {
                id: "dup".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: super::app::ToolCardStatus::Done,
                output: Some("one".into()),
                truncated: false,
                exit: None,
            }),
        ];
        store.write(&meta, &history, &transcript, None).unwrap();

        let mut state = super::app::AppState::new();
        state.open_session_picker(vec![SessionSummary {
            id: "sess-picker".to_string(),
            created_at: "1".to_string(),
            first_user: None,
        }]);
        let mut pending_str = String::new();
        assert_eq!(
            handle_session_picker_batch_key(&mut state, key(KeyCode::Enter), &mut pending_str),
            Some(super::app::ApplyBatchKeyResult::BreakBatch)
        );
        let selected = state
            .take_pending_session_switch()
            .expect("picker selected session");
        let mut active_meta = session_meta("fresh-session");
        let (activation_tx, mut activation_rx) = mpsc::unbounded_channel();
        activate_session_switch(
            &store,
            &selected,
            &mut state,
            &mut active_meta,
            &activation_tx,
        )
        .await;
        assert_eq!(active_meta.id, "sess-picker");
        let restore_input = activation_rx.try_recv().expect("picker restore command");
        assert!(matches!(
            &restore_input,
            UserInput::SetProvider {
                kind: ProviderSwitchKind::SessionRestore,
                ..
            }
        ));
        let h = state.agent_history.lock().await.clone();
        let tr = state.transcript.clone();
        assert!(
            h.iter().any(|m| matches!(
                m,
                Message::ToolResult {
                    content,
                    is_error: true,
                    ..
                } if content == INTERRUPTED_TOOL_CONTENT
            )),
            "dangling occurrence filled"
        );
        // Running 卡 → Error；Done 不变
        assert!(matches!(
            &tr[0],
            TranscriptBlock::Tool(c)
                if c.status == super::app::ToolCardStatus::Error
                    && c.output.as_deref() == Some(PREV_SESSION_INTERRUPTED_OUTPUT)
        ));
        assert!(matches!(
            &tr[1],
            TranscriptBlock::Tool(c) if c.status == super::app::ToolCardStatus::Done
        ));

        let recording = Arc::new(MockProvider::new(vec![
            ModelResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "dup".into(),
                    name: "read_file".into(),
                    arguments: json!({"path": "missing-picker-file.txt"}),
                }],
                finish_reason: FinishReason::ToolCalls,
                usage: None,
                thinking: Vec::new(),
            },
            ModelResponse {
                text: "ok".into(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
                thinking: Vec::new(),
            },
        ]));
        let assembled = assemble_agent(
            recording.clone(),
            &config(),
            Box::new(normal_channel_decider(mpsc::unbounded_channel().0)),
            None,
            None,
            None,
        );
        assert!(
            h.iter().any(|m| matches!(
                m,
                Message::Assistant { thinking: t, .. } if t == &thinking
            )),
            "审查#3/#6：picker restore 保留 thinking"
        );

        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (_i_tx, i_rx) = mpsc::unbounded_channel();
        let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
        let handle = tokio::spawn(run_agent_task(
            assembled.agent,
            state.agent_history.clone(),
            assembled.compacting,
            RunAgentTaskConfig {
                // persisted provider 未注册，真实恢复命令失败后继续用 RecordingProvider。
                profiles: BTreeMap::new(),
                startup_config: config(),
                credentials_path: temp.path().join("credentials"),
                tool_ctx: ToolContext {
                    cwd: temp.path().to_path_buf(),
                    max_output_bytes: 4096,
                },
            },
            input_rx,
            i_rx,
            ui_tx,
        ));
        input_tx.send(restore_input).unwrap();
        input_tx.send(UserInput::Prompt("go".into())).unwrap();
        let mut fallback_notices = 0usize;
        loop {
            let event = ui_rx.recv().await.expect("ui event");
            let done = matches!(event, AgentEvent::TurnComplete);
            if matches!(&event, AgentEvent::Notice(message) if message.contains("未知 provider"))
            {
                fallback_notices += 1;
            }
            state.apply(event);
            if done {
                break;
            }
        }
        drop(input_tx);
        handle.await.unwrap();
        assert_eq!(fallback_notices, 1);
        let recorded = recording.recorded_requests();
        assert_eq!(recorded.len(), 2);
        let msgs = &recorded[0].messages;
        assert!(matches!(&msgs[0], Message::System(s) if s == DEFAULT_SYSTEM_PROMPT));
        assert!(msgs.iter().any(|m| matches!(
            m,
            Message::Assistant { thinking: t, .. } if t == &thinking
        )));
        assert_eq!(
            msgs.iter()
                .filter(|m| matches!(m, Message::ToolResult { call_id, .. } if call_id == "dup"))
                .count(),
            2,
            "picker 首次 Provider 请求必须按 occurrence 补齐重复 id"
        );

        let cards: Vec<_> = state
            .transcript
            .iter()
            .filter_map(|block| match block {
                TranscriptBlock::Tool(card) if card.id == "dup" => Some(card),
                _ => None,
            })
            .collect();
        assert_eq!(cards.len(), 3, "两张历史卡 + 一张新 turn 卡");
        assert_eq!(cards[0].status, super::app::ToolCardStatus::Error);
        assert_eq!(
            cards[0].output.as_deref(),
            Some(PREV_SESSION_INTERRUPTED_OUTPUT)
        );
        assert_eq!(cards[1].status, super::app::ToolCardStatus::Done);
        assert_eq!(cards[1].output.as_deref(), Some("one"));
        assert_ne!(cards[2].status, super::app::ToolCardStatus::Running);
    }

    /// 审查 #6.5：新 turn 复用 call id 只更新新 Running 卡，历史 Error/Done 不变。
    #[test]
    fn new_turn_same_call_id_updates_only_new_running_card() {
        use super::app::{ToolCard, ToolCardStatus};

        let mut state = super::app::AppState::new();
        state.transcript = vec![
            TranscriptBlock::Tool(ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: ToolCardStatus::Error,
                output: Some(PREV_SESSION_INTERRUPTED_OUTPUT.into()),
                truncated: false,
                exit: None,
            }),
            TranscriptBlock::Tool(ToolCard {
                id: "call-1".into(),
                name: "read_file".into(),
                args: json!({}),
                readonly: true,
                status: ToolCardStatus::Done,
                output: Some("old-done".into()),
                truncated: false,
                exit: None,
            }),
        ];
        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".into(),
            name: "read_file".into(),
            args: json!({}),
            readonly: true,
        });

        assert_eq!(state.transcript.len(), 3);
        assert!(matches!(
            &state.transcript[0],
            TranscriptBlock::Tool(c)
                if c.status == ToolCardStatus::Error
                    && c.output.as_deref() == Some(PREV_SESSION_INTERRUPTED_OUTPUT)
        ));
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Tool(c)
                if c.status == ToolCardStatus::Done && c.output.as_deref() == Some("old-done")
        ));
        assert!(matches!(
            &state.transcript[2],
            TranscriptBlock::Tool(c) if c.status == ToolCardStatus::Running && c.id == "call-1"
        ));
    }
}
