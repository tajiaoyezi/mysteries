use crate::agent::message::Message;
use crate::agent::AgentStatus;
use crate::permission::PermissionDecision;
use crate::provider::Usage;
use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
use crate::tui::command::{command_metadata, parse_command, Command, CommandMetadata};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const ASCII_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Busy,
    CallingModel,
    ExecutingTool(String),
    WaitingForPermission,
}

impl Phase {
    pub fn is_running(&self) -> bool {
        !matches!(self, Phase::Ready)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptBlock {
    User(String),
    Assistant(String),
    Tool(ToolCard),
    Error(String),
    Help,
    Status(StatusSnapshot),
    Notice(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub provider: String,
    pub model: String,
    pub max_iterations: u32,
    pub cwd: PathBuf,
    pub tools: usize,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            max_iterations: 4,
            cwd: PathBuf::from("."),
            tools: 7,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub provider: String,
    pub model: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub messages: usize,
    pub cwd: PathBuf,
    pub tools: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCardStatus {
    Running,
    Done,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub readonly: bool,
    pub status: ToolCardStatus,
    pub output: Option<String>,
    pub truncated: bool,
    pub exit: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandCompletion {
    pub candidates: Vec<CommandMetadata>,
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Add,
    Del,
    Ctx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

pub fn compute_diff(tool_name: &str, args: &Value) -> Vec<DiffLine> {
    match tool_name {
        "write_file" => diff_lines(args.get("content"), DiffKind::Add),
        "edit_file" => {
            let mut lines = diff_lines(args.get("old_string"), DiffKind::Del);
            lines.extend(diff_lines(args.get("new_string"), DiffKind::Add));
            lines
        }
        _ => Vec::new(),
    }
}

fn diff_lines(value: Option<&Value>, kind: DiffKind) -> Vec<DiffLine> {
    value
        .and_then(Value::as_str)
        .map(|text| {
            text.lines()
                .map(|line| DiffLine {
                    kind: kind.clone(),
                    text: line.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub const ESTIMATED_CHARS_PER_TOKEN: u32 = 4;

pub fn estimate_tokens_from_chars(chars: u32) -> u32 {
    chars / ESTIMATED_CHARS_PER_TOKEN
}

pub fn estimate_streaming_rate_tps(chars: u32, elapsed: Duration) -> Option<f64> {
    if elapsed.is_zero() {
        return None;
    }
    Some(estimate_tokens_from_chars(chars) as f64 / elapsed.as_secs_f64())
}

pub struct AppState {
    pub session: SessionSnapshot,
    pub agent_history: Arc<Mutex<Vec<Message>>>,
    pub iteration: u32,
    pub transcript: Vec<TranscriptBlock>,
    pub tools_expanded: bool,
    pub command_completion: Option<CommandCompletion>,
    pub input: String,
    pub phase: Phase,
    pub pending_permission: Option<PermissionRequest>,
    pub scroll_offset: usize,
    pub spinner_frame: usize,
    pub should_exit: bool,
    follows_bottom: bool,
    turn_output_tokens: u32,
    last_rate_tps: Option<f64>,
    streaming_chars_this_call: u32,
    last_rate_is_approximate: bool,
    idle_output_tokens: u32,
    idle_rate_tps: Option<f64>,
    idle_rate_is_approximate: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_session(SessionSnapshot::default())
    }

    pub fn with_session(session: SessionSnapshot) -> Self {
        Self::with_session_and_history(
            session,
            Arc::new(Mutex::new(vec![Message::System(
                crate::agent::DEFAULT_SYSTEM_PROMPT.to_string(),
            )])),
        )
    }

    pub fn with_session_and_history(
        session: SessionSnapshot,
        agent_history: Arc<Mutex<Vec<Message>>>,
    ) -> Self {
        Self {
            session,
            agent_history,
            iteration: 0,
            transcript: Vec::new(),
            tools_expanded: false,
            command_completion: None,
            input: String::new(),
            phase: Phase::Ready,
            pending_permission: None,
            scroll_offset: 0,
            spinner_frame: 0,
            should_exit: false,
            follows_bottom: true,
            turn_output_tokens: 0,
            last_rate_tps: None,
            streaming_chars_this_call: 0,
            last_rate_is_approximate: false,
            idle_output_tokens: 0,
            idle_rate_tps: None,
            idle_rate_is_approximate: false,
        }
    }

    pub fn output_tokens_this_turn(&self) -> u32 {
        self.turn_output_tokens
    }

    pub fn last_rate_tps(&self) -> Option<f64> {
        self.last_rate_tps
    }

    pub fn record_usage(&mut self, usage: Usage, elapsed: Duration) {
        self.turn_output_tokens += usage.output_tokens;
        self.last_rate_tps = if elapsed.is_zero() {
            None
        } else {
            Some(usage.output_tokens as f64 / elapsed.as_secs_f64())
        };
        self.last_rate_is_approximate = false;
    }

    pub fn record_streaming_chars(&mut self, chars: u32, elapsed: Duration) {
        self.streaming_chars_this_call += chars;
        self.last_rate_tps = estimate_streaming_rate_tps(self.streaming_chars_this_call, elapsed);
        self.last_rate_is_approximate = true;
    }

    pub fn reset_streaming_chars_for_call(&mut self) {
        self.streaming_chars_this_call = 0;
    }

    pub fn idle_output_tokens(&self) -> u32 {
        self.idle_output_tokens
    }

    pub fn idle_rate_tps(&self) -> Option<f64> {
        self.idle_rate_tps
    }

    pub fn idle_rate_is_approximate(&self) -> bool {
        self.idle_rate_is_approximate
    }

    pub fn last_rate_is_approximate(&self) -> bool {
        self.last_rate_is_approximate
    }

    fn reset_turn_token_usage(&mut self) {
        self.turn_output_tokens = 0;
        self.last_rate_tps = None;
        self.last_rate_is_approximate = false;
        self.streaming_chars_this_call = 0;
    }

    fn save_idle_token_summary(&mut self) {
        self.idle_output_tokens = self.turn_output_tokens;
        self.idle_rate_tps = self.last_rate_tps;
        self.idle_rate_is_approximate = self.last_rate_is_approximate;
    }

    pub fn status_snapshot(&self) -> StatusSnapshot {
        StatusSnapshot {
            provider: self.session.provider.clone(),
            model: self.session.model.clone(),
            iteration: self.iteration,
            max_iterations: self.session.max_iterations,
            messages: self.dialog_message_count(),
            cwd: self.session.cwd.clone(),
            tools: self.session.tools,
        }
    }

    pub fn dialog_message_count(&self) -> usize {
        self.transcript
            .iter()
            .filter(|block| {
                matches!(
                    block,
                    TranscriptBlock::User(_) | TranscriptBlock::Assistant(_)
                )
            })
            .count()
    }

    pub fn visible_scroll_offset(&self, total_lines: usize, viewport_lines: usize) -> usize {
        if self.follows_bottom {
            bottom_offset(total_lines, viewport_lines)
        } else {
            self.scroll_offset
                .min(bottom_offset(total_lines, viewport_lines))
        }
    }

    pub fn page_up(&mut self, total_lines: usize, viewport_lines: usize) {
        let current = self.visible_scroll_offset(total_lines, viewport_lines);
        self.scroll_offset = current.saturating_sub(viewport_lines);
        self.follows_bottom = false;
    }

    pub fn page_down(&mut self, total_lines: usize, viewport_lines: usize) {
        let bottom = bottom_offset(total_lines, viewport_lines);
        let next = self
            .visible_scroll_offset(total_lines, viewport_lines)
            .saturating_add(viewport_lines)
            .min(bottom);
        self.scroll_offset = next;
        self.follows_bottom = next == bottom;
    }

    pub fn scroll_to_top(&mut self, _total_lines: usize, _viewport_lines: usize) {
        self.scroll_offset = 0;
        self.follows_bottom = false;
    }

    pub fn scroll_to_bottom(&mut self, _total_lines: usize, _viewport_lines: usize) {
        self.follows_bottom = true;
    }

    pub fn scroll_up(&mut self, total_lines: usize, viewport_lines: usize, lines: usize) {
        let current = self.visible_scroll_offset(total_lines, viewport_lines);
        self.scroll_offset = current.saturating_sub(lines);
        self.follows_bottom = false;
    }

    pub fn scroll_down(&mut self, total_lines: usize, viewport_lines: usize, lines: usize) {
        let bottom = bottom_offset(total_lines, viewport_lines);
        let next = self
            .visible_scroll_offset(total_lines, viewport_lines)
            .saturating_add(lines)
            .min(bottom);
        self.scroll_offset = next;
        self.follows_bottom = next == bottom;
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner_glyph(&self) -> &'static str {
        spinner_glyph(self.spinner_frame, true)
    }

    pub fn toggle_tools_expanded(&mut self) {
        self.tools_expanded = !self.tools_expanded;
    }

    fn refresh_command_completion(&mut self) {
        let previous = self
            .command_completion
            .as_ref()
            .and_then(|completion| completion.candidates.get(completion.selected))
            .map(|command| command.name);

        let Some(mut candidates) = command_completion_candidates(&self.input) else {
            self.command_completion = None;
            return;
        };

        let selected = previous
            .and_then(|name| candidates.iter().position(|command| command.name == name))
            .unwrap_or(0);

        self.command_completion = Some(CommandCompletion {
            candidates: std::mem::take(&mut candidates),
            selected,
        });
    }

    fn close_command_completion(&mut self) {
        self.command_completion = None;
    }

    fn move_command_completion_selection(&mut self, delta: isize) {
        let Some(completion) = self.command_completion.as_mut() else {
            return;
        };
        if completion.candidates.is_empty() {
            completion.selected = 0;
            return;
        }

        let len = completion.candidates.len();
        completion.selected = if delta.is_negative() {
            completion.selected.checked_sub(1).unwrap_or(len - 1)
        } else {
            (completion.selected + 1) % len
        };
    }

    fn complete_selected_command(&mut self) {
        let Some(completion) = self.command_completion.as_ref() else {
            return;
        };
        let Some(command) = completion.candidates.get(completion.selected) else {
            return;
        };

        self.input = command.name.to_string();
        self.close_command_completion();
    }

    fn handle_command_completion_key(&mut self, key: KeyEvent) -> bool {
        if self.command_completion.is_none() {
            return false;
        }

        match key.code {
            KeyCode::Up => {
                self.move_command_completion_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_command_completion_selection(1);
                true
            }
            KeyCode::Tab | KeyCode::Enter => {
                self.complete_selected_command();
                true
            }
            KeyCode::Esc => {
                self.close_command_completion();
                true
            }
            _ => false,
        }
    }

    pub fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                if self.phase == Phase::Ready {
                    self.phase = Phase::Busy;
                }
                match self.transcript.last_mut() {
                    Some(TranscriptBlock::Assistant(current)) => current.push_str(&text),
                    _ => self.transcript.push(TranscriptBlock::Assistant(text)),
                }
            }
            AgentEvent::ToolCallStarted {
                id,
                name,
                args,
                readonly,
            } => {
                self.transcript.push(TranscriptBlock::Tool(ToolCard {
                    id,
                    name,
                    args,
                    readonly,
                    status: ToolCardStatus::Running,
                    output: None,
                    truncated: false,
                    exit: None,
                }));
            }
            AgentEvent::ToolCallFinished { id, outcome } => {
                let status = if outcome.is_error {
                    ToolCardStatus::Error
                } else {
                    ToolCardStatus::Done
                };

                if let Some(card) = self.transcript.iter_mut().find_map(|block| match block {
                    TranscriptBlock::Tool(card) if card.id == id => Some(card),
                    _ => None,
                }) {
                    card.status = status;
                    card.output = Some(outcome.content);
                    card.truncated = outcome.truncated;
                    card.exit = outcome.exit;
                }
            }
            AgentEvent::StatusChanged(status) => {
                if status == AgentStatus::CallingModel {
                    self.iteration += 1;
                }
                self.phase = match status {
                    AgentStatus::Idle => Phase::Ready,
                    AgentStatus::CallingModel => Phase::CallingModel,
                    AgentStatus::ExecutingTool(name) => Phase::ExecutingTool(name),
                    AgentStatus::WaitingForPermission => Phase::WaitingForPermission,
                };
            }
            AgentEvent::PermissionRequired(request) => {
                self.pending_permission = Some(request);
                self.phase = Phase::WaitingForPermission;
            }
            AgentEvent::TurnComplete => {
                self.pending_permission = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.save_idle_token_summary();
                self.reset_turn_token_usage();
            }
            AgentEvent::Notice(message) => {
                self.transcript.push(TranscriptBlock::Notice(message));
            }
            AgentEvent::Interrupted => {
                self.pending_permission = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.transcript
                    .push(TranscriptBlock::Notice("⊘ 已中断本轮".to_string()));
            }
            AgentEvent::Error(message) => {
                self.pending_permission = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.transcript.push(TranscriptBlock::Error(message));
            }
            AgentEvent::Usage {
                input_tokens: _,
                output_tokens: _,
            } => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, input_tx: &mpsc::UnboundedSender<UserInput>) {
        self.on_key_inner(key, input_tx, None);
    }

    pub fn on_key_with_interrupt(
        &mut self,
        key: KeyEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
        interrupt_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        self.on_key_inner(key, input_tx, Some(interrupt_tx));
    }

    fn on_key_inner(
        &mut self,
        key: KeyEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
        interrupt_tx: Option<&mpsc::UnboundedSender<UserInput>>,
    ) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.toggle_tools_expanded();
            return;
        }

        if self.pending_permission.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.answer_pending_permission(PermissionDecision::Allow);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.answer_pending_permission(PermissionDecision::Deny);
                }
                _ => {}
            }
            return;
        }

        if self.handle_command_completion_key(key) {
            return;
        }

        if key.code == KeyCode::Esc && self.phase.is_running() {
            if let Some(tx) = interrupt_tx {
                let _ = tx.send(UserInput::Interrupt);
            }
            return;
        }

        match key.code {
            KeyCode::Char(ch) => {
                self.input.push(ch);
                self.refresh_command_completion();
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.refresh_command_completion();
            }
            KeyCode::Enter => {
                self.close_command_completion();
                let prompt = self.input.trim().to_string();
                if prompt.is_empty() {
                    return;
                }
                self.input.clear();
                if let Some(command) = parse_command(&prompt) {
                    self.execute_command(command, input_tx);
                    return;
                }
                self.reset_turn_token_usage();
                self.iteration = 0;
                self.phase = Phase::Busy;
                self.transcript.push(TranscriptBlock::User(prompt.clone()));
                let _ = input_tx.send(UserInput::Prompt(prompt));
            }
            _ => {}
        }
    }

    pub fn execute_command(
        &mut self,
        command: Command,
        _input_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        match command {
            Command::Help => self.transcript.push(TranscriptBlock::Help),
            Command::Clear => self.transcript.clear(),
            Command::Status => self
                .transcript
                .push(TranscriptBlock::Status(self.status_snapshot())),
            Command::Exit => self.should_exit = true,
            Command::Login => self.transcript.push(TranscriptBlock::Notice(
                "凭据占位:请通过 config / 环境变量配置 API key。".to_string(),
            )),
            Command::Logout => self.transcript.push(TranscriptBlock::Notice(
                "凭据占位:当前版本未保存登录态,无需 logout。".to_string(),
            )),
            Command::Unknown(name) => self
                .transcript
                .push(TranscriptBlock::Notice(format!("未知命令: /{name}"))),
            Command::Model(None) => {
                self.transcript.push(TranscriptBlock::Notice(format!(
                    "当前 model: {}",
                    self.session.model
                )));
            }
            Command::Model(Some(model)) => {
                self.session.model = model.clone();
                let _ = _input_tx.send(UserInput::SetModel(model));
            }
            Command::Compact => {
                let _ = _input_tx.send(UserInput::Compact);
            }
        }
    }

    fn answer_pending_permission(&mut self, decision: PermissionDecision) {
        if let Some(request) = self.pending_permission.take() {
            let _ = request.responder.send(decision);
            self.phase = Phase::Busy;
        }
    }
}

fn command_completion_candidates(input: &str) -> Option<Vec<CommandMetadata>> {
    if !input.starts_with('/') {
        return None;
    }
    if input.chars().any(char::is_whitespace) {
        return None;
    }

    let candidates = command_metadata()
        .iter()
        .copied()
        .filter(|command| command.name.starts_with(input))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

fn bottom_offset(total_lines: usize, viewport_lines: usize) -> usize {
    total_lines.saturating_sub(viewport_lines)
}

fn spinner_glyph(frame: usize, unicode: bool) -> &'static str {
    if unicode {
        SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]
    } else {
        ASCII_SPINNER_FRAMES[frame % ASCII_SPINNER_FRAMES.len()]
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_diff, estimate_streaming_rate_tps, estimate_tokens_from_chars, AppState, DiffKind,
        DiffLine, Phase, SessionSnapshot, StatusSnapshot, ToolCard, ToolCardStatus,
        TranscriptBlock,
    };
    use crate::agent::AgentStatus;
    use crate::permission::PermissionDecision;
    use crate::provider::Usage;
    use crate::tool::ToolOutcome;
    use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use serde_json::json;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_kind(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)
    }

    fn key_with_modifiers_and_kind(
        code: KeyCode,
        modifiers: KeyModifiers,
        kind: KeyEventKind,
    ) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, kind)
    }

    fn diff_line(kind: DiffKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
        }
    }

    fn session() -> SessionSnapshot {
        SessionSnapshot {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            max_iterations: 8,
            cwd: PathBuf::from("workspace"),
            tools: 7,
        }
    }

    #[test]
    fn compute_diff_derives_write_edit_and_shell_from_args_without_reading_files() {
        assert_eq!(
            compute_diff(
                "write_file",
                &json!({ "path": "does-not-need-to-exist.txt", "content": "one\ntwo" }),
            ),
            vec![
                diff_line(DiffKind::Add, "one"),
                diff_line(DiffKind::Add, "two")
            ]
        );

        assert_eq!(
            compute_diff(
                "edit_file",
                &json!({
                    "path": "does-not-need-to-exist.txt",
                    "old_string": "old a\nold b",
                    "new_string": "new a\nnew b"
                }),
            ),
            vec![
                diff_line(DiffKind::Del, "old a"),
                diff_line(DiffKind::Del, "old b"),
                diff_line(DiffKind::Add, "new a"),
                diff_line(DiffKind::Add, "new b"),
            ]
        );

        assert_eq!(
            compute_diff("run_shell", &json!({ "command": "cargo test" })),
            Vec::new()
        );
    }

    #[test]
    fn scroll_offset_follows_bottom_preserves_manual_position_and_clamps() {
        let mut state = AppState::new();

        assert_eq!(state.visible_scroll_offset(20, 5), 15);

        state.page_up(20, 5);
        assert_eq!(state.scroll_offset, 10);
        assert_eq!(state.visible_scroll_offset(30, 5), 10);

        state.page_up(30, 5);
        state.page_up(30, 5);
        state.page_up(30, 5);
        assert_eq!(state.visible_scroll_offset(30, 5), 0);

        state.page_down(30, 5);
        assert_eq!(state.visible_scroll_offset(30, 5), 5);

        for _ in 0..10 {
            state.page_down(30, 5);
        }
        assert_eq!(state.visible_scroll_offset(30, 5), 25);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
    }

    #[test]
    fn line_scroll_moves_in_small_steps_and_refollows_at_bottom() {
        let mut state = AppState::new();

        state.scroll_up(30, 5, 3);
        assert_eq!(state.visible_scroll_offset(30, 5), 22);
        assert_eq!(state.visible_scroll_offset(40, 5), 22);

        state.scroll_down(40, 5, 2);
        assert_eq!(state.visible_scroll_offset(40, 5), 24);

        state.scroll_down(40, 5, 20);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(state.visible_scroll_offset(50, 5), 45);
    }

    #[test]
    fn boundary_scroll_jumps_to_top_and_returns_to_followed_bottom() {
        let mut state = AppState::new();

        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert!(state.follows_bottom);

        state.scroll_up(40, 5, 7);
        assert_eq!(state.scroll_offset, 28);
        assert_eq!(state.visible_scroll_offset(40, 5), 28);
        assert!(!state.follows_bottom);

        state.scroll_to_top(40, 5);
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert!(!state.follows_bottom);

        state.scroll_to_bottom(40, 5);
        assert!(state.follows_bottom);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
    }

    #[test]
    fn advance_spinner_cycles_frame_index() {
        let mut state = AppState::new();

        for expected in 1..=9 {
            state.advance_spinner();
            assert_eq!(state.spinner_frame, expected);
        }

        state.advance_spinner();
        assert_eq!(state.spinner_frame, 0);
    }

    #[test]
    fn app_state_tracks_session_snapshot_and_iteration_counter() {
        let mut state = AppState::with_session(session());
        let (tx, _rx) = mpsc::unbounded_channel();

        assert_eq!(state.session.provider, "anthropic");
        assert_eq!(state.session.model, "claude-test");
        assert_eq!(state.session.max_iterations, 8);
        assert_eq!(state.session.cwd, PathBuf::from("workspace"));
        assert_eq!(state.session.tools, 7);

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 1);
        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 2);

        state.apply(AgentEvent::TurnComplete);
        assert_eq!(state.iteration, 0);

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 1);
        state.input = "next prompt".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.iteration, 0);
    }

    #[test]
    fn slash_commands_clear_help_status_exit_and_do_not_submit_prompt() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());
        state
            .transcript
            .push(TranscriptBlock::User("old".to_string()));

        state.input = "/clear".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(state.transcript.is_empty());
        assert!(rx.try_recv().is_err());

        state.input = "/help".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.transcript, vec![TranscriptBlock::Help]);

        state.iteration = 3;
        state.input = "/status".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(
            state.transcript.last(),
            Some(&TranscriptBlock::Status(StatusSnapshot {
                provider: "anthropic".to_string(),
                model: "claude-test".to_string(),
                iteration: 3,
                max_iterations: 8,
                messages: 0,
                cwd: PathBuf::from("workspace"),
                tools: 7,
            }))
        );

        state.input = "/exit".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(state.should_exit);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn placeholder_and_unknown_commands_append_notice_without_agent_input() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());

        for input in ["/login", "/logout", "/xyz"] {
            state.input = input.to_string();
            state.on_key(key(KeyCode::Enter), &tx);
        }

        assert!(matches!(
            &state.transcript[0],
            TranscriptBlock::Notice(text) if text.contains("凭据")
        ));
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Notice(text) if text.contains("凭据")
        ));
        assert!(matches!(
            &state.transcript[2],
            TranscriptBlock::Notice(text) if text.contains("未知命令") && text.contains("xyz")
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn status_snapshot_counts_only_user_and_assistant_dialog_messages() {
        let mut state = AppState::with_session(session());
        state
            .transcript
            .push(TranscriptBlock::User("u1".to_string()));
        state.transcript.push(TranscriptBlock::Tool(ToolCard {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
            status: ToolCardStatus::Done,
            output: Some("tool output".to_string()),
            truncated: false,
            exit: None,
        }));
        state.transcript.push(TranscriptBlock::Help);
        state
            .transcript
            .push(TranscriptBlock::Assistant("a1".to_string()));
        state
            .transcript
            .push(TranscriptBlock::Notice("notice".to_string()));
        state
            .transcript
            .push(TranscriptBlock::Error("fatal".to_string()));

        assert_eq!(state.status_snapshot().messages, 2);
    }

    #[test]
    fn model_command_shows_current_model_or_sends_set_model() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());

        state.input = "/model".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(matches!(
            state.transcript.last(),
            Some(TranscriptBlock::Notice(text))
                if text.contains("claude-test") && text.contains("model")
        ));

        state.input = "/model claude-next".to_string();
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.session.model, "claude-next");
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::SetModel("claude-next".to_string())
        );
    }

    #[test]
    fn apply_tool_call_started_adds_running_tool_block() {
        let mut state = AppState::new();

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
        });

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::Tool(ToolCard {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                args: json!({ "path": "note.txt" }),
                readonly: true,
                status: ToolCardStatus::Running,
                output: None,
                truncated: false,
                exit: None,
            })]
        );
    }

    #[test]
    fn tool_events_are_inserted_into_transcript_timeline() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("inspect config".to_string()));

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "src/config.rs" }),
            readonly: true,
        });

        assert_eq!(state.transcript.len(), 2);
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Tool(card)
                if card.id == "call-1" && card.status == ToolCardStatus::Running
        ));

        state.apply(AgentEvent::ToolCallFinished {
            id: "call-1".to_string(),
            outcome: ToolOutcome {
                content: "config contents".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
        });

        assert_eq!(state.transcript.len(), 2);
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Tool(card)
                if card.status == ToolCardStatus::Done
                    && card.output.as_deref() == Some("config contents")
        ));
    }

    #[test]
    fn apply_tool_call_finished_updates_tool_block_to_done_or_error() {
        let mut state = AppState::new();
        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "write_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: false,
        });
        state.apply(AgentEvent::ToolCallFinished {
            id: "call-1".to_string(),
            outcome: ToolOutcome {
                content: "wrote note.txt".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
        });

        let TranscriptBlock::Tool(card) = &state.transcript[0] else {
            panic!("expected tool block");
        };
        assert_eq!(card.status, ToolCardStatus::Done);
        assert_eq!(card.output.as_deref(), Some("wrote note.txt"));

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-2".to_string(),
            name: "run_shell".to_string(),
            args: json!({ "command": "false" }),
            readonly: false,
        });
        state.apply(AgentEvent::ToolCallFinished {
            id: "call-2".to_string(),
            outcome: ToolOutcome {
                content: "command failed: permission denied".to_string(),
                is_error: true,
                truncated: true,
                exit: None,
            },
        });

        let TranscriptBlock::Tool(card) = &state.transcript[1] else {
            panic!("expected tool block");
        };
        assert_eq!(card.status, ToolCardStatus::Error);
        assert_eq!(
            card.output.as_deref(),
            Some("command failed: permission denied")
        );
        assert!(card.truncated);
    }

    #[test]
    fn apply_tool_call_finished_without_matching_started_tool_is_ignored() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("keep me".to_string()));

        state.apply(AgentEvent::ToolCallFinished {
            id: "missing".to_string(),
            outcome: ToolOutcome {
                content: "ignored".to_string(),
                is_error: false,
                truncated: false,
                exit: Some(0),
            },
        });

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("keep me".to_string())]
        );
    }

    #[test]
    fn apply_status_changed_updates_full_phase() {
        let mut state = AppState::new();

        state.apply(AgentEvent::StatusChanged(AgentStatus::ExecutingTool(
            "write_file".to_string(),
        )));

        assert_eq!(state.phase, Phase::ExecutingTool("write_file".to_string()));
    }

    #[test]
    fn apply_text_delta_accumulates_current_assistant_block() {
        let mut state = AppState::new();

        state.apply(AgentEvent::TextDelta("hello".to_string()));
        state.apply(AgentEvent::TextDelta(" world".to_string()));

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::Assistant("hello world".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
    }

    #[test]
    fn apply_permission_request_sets_pending_and_waiting_phase() {
        let (tx, _rx) = oneshot::channel();
        let mut state = AppState::new();

        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({ "path": "note.txt" }),
            responder: tx,
        }));

        assert_eq!(state.phase, Phase::WaitingForPermission);
        let pending = state.pending_permission.as_ref().unwrap();
        assert_eq!(pending.tool_name, "write_file");
        assert_eq!(pending.args, json!({ "path": "note.txt" }));
    }

    #[test]
    fn on_key_edits_text_and_enter_submits_prompt() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('h')), &tx);
        state.on_key(key(KeyCode::Char('i')), &tx);
        state.on_key(key(KeyCode::Backspace), &tx);
        state.on_key(key(KeyCode::Char('!')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);

        assert_eq!(state.input, "");
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("h!".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
        assert_eq!(rx.try_recv().unwrap(), UserInput::Prompt("h!".to_string()));
    }

    #[test]
    fn on_key_ignores_non_press_events_to_avoid_duplicate_input() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key_with_kind(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        state.on_key(
            key_with_kind(KeyCode::Char('a'), KeyEventKind::Release),
            &tx,
        );
        state.on_key(key_with_kind(KeyCode::Char('a'), KeyEventKind::Repeat), &tx);

        assert_eq!(state.input, "a");
    }

    #[test]
    fn ctrl_o_is_reserved_for_tool_expansion_and_never_enters_input() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        assert!(!state.tools_expanded);

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(
            state.tools_expanded,
            "ctrl+o Press should expand folded tool cards"
        );
        assert_eq!(
            state.input, "",
            "ctrl+o should toggle tool expansion instead of typing 'o'"
        );
        assert!(rx.try_recv().is_err());

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(
            !state.tools_expanded,
            "second ctrl+o Press should fold tool cards again"
        );
        assert_eq!(state.input, "");

        state.input = "keep".to_string();
        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Release,
            ),
            &tx,
        );
        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Repeat,
            ),
            &tx,
        );

        assert!(!state.tools_expanded);
        assert_eq!(state.input, "keep");
    }

    #[test]
    fn ctrl_o_toggles_during_pending_permission_without_answering_it() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: allow_tx,
        }));

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &input_tx,
        );

        assert!(state.tools_expanded);
        assert!(state.pending_permission.is_some());
        assert_eq!(state.phase, Phase::WaitingForPermission);
        assert!(allow_rx.try_recv().is_err());

        state.on_key(key(KeyCode::Char('y')), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionDecision::Allow);
        assert!(state.pending_permission.is_none());
        assert_eq!(state.phase, Phase::Busy);
        assert!(state.tools_expanded);
    }

    #[test]
    fn ctrl_o_toggles_during_running_phase_without_interrupting_escape() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.on_key_with_interrupt(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &input_tx,
            &interrupt_tx,
        );

        assert!(state.tools_expanded);
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(input_rx.try_recv().is_err());
        assert!(interrupt_rx.try_recv().is_err());

        state.on_key_with_interrupt(key(KeyCode::Esc), &input_tx, &interrupt_tx);

        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(state.tools_expanded);
    }

    fn completion_names(state: &AppState) -> Vec<&'static str> {
        state
            .command_completion
            .as_ref()
            .expect("completion should be visible")
            .candidates
            .iter()
            .map(|command| command.name)
            .collect()
    }

    fn selected_completion_name(state: &AppState) -> &'static str {
        let completion = state
            .command_completion
            .as_ref()
            .expect("completion should be visible");
        completion.candidates[completion.selected].name
    }

    #[test]
    fn slash_completion_filters_candidates_and_completes_selection() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('/')), &tx);
        assert_eq!(
            completion_names(&state),
            vec!["/help", "/clear", "/model", "/status", "/exit", "/login", "/logout", "/compact"]
        );
        assert_eq!(selected_completion_name(&state), "/help");

        state.on_key(key(KeyCode::Char('c')), &tx);
        assert_eq!(completion_names(&state), vec!["/clear", "/compact"]);
        assert_eq!(selected_completion_name(&state), "/clear");

        state.on_key(key(KeyCode::Down), &tx);
        assert_eq!(selected_completion_name(&state), "/compact");

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(selected_completion_name(&state), "/clear");

        state.on_key(key(KeyCode::Tab), &tx);
        assert_eq!(state.input, "/clear");
        assert!(state.command_completion.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn slash_completion_hides_for_arguments_plain_prompt_and_escape() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.input = "/model ".to_string();
        state.on_key(key(KeyCode::Char('g')), &tx);
        assert!(state.command_completion.is_none());

        state.input = "hello".to_string();
        state.on_key(key(KeyCode::Char('!')), &tx);
        assert!(state.command_completion.is_none());

        state.input.clear();
        state.on_key(key(KeyCode::Char('/')), &tx);
        assert!(state.command_completion.is_some());
        state.on_key(key(KeyCode::Esc), &tx);
        assert_eq!(state.input, "/");
        assert!(state.command_completion.is_none());
    }

    #[test]
    fn on_key_escape_interrupts_running_turn_on_dedicated_channel_only_for_press() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.on_key_with_interrupt(key(KeyCode::Esc), &input_tx, &interrupt_tx);

        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert!(input_rx.try_recv().is_err());
        assert_eq!(state.phase, Phase::CallingModel);

        state.on_key_with_interrupt(
            key_with_kind(KeyCode::Esc, KeyEventKind::Release),
            &input_tx,
            &interrupt_tx,
        );
        state.on_key_with_interrupt(
            key_with_kind(KeyCode::Esc, KeyEventKind::Repeat),
            &input_tx,
            &interrupt_tx,
        );

        assert!(interrupt_rx.try_recv().is_err());
    }

    #[test]
    fn on_key_answers_pending_permission_with_y_or_n() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut allow_state = AppState::new();
        allow_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: allow_tx,
        }));

        allow_state.on_key(key(KeyCode::Char('y')), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionDecision::Allow);
        assert!(allow_state.pending_permission.is_none());
        assert_eq!(allow_state.phase, Phase::Busy);

        let (deny_tx, mut deny_rx) = oneshot::channel();
        let mut deny_state = AppState::new();
        deny_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: deny_tx,
        }));

        deny_state.on_key(key(KeyCode::Char('n')), &input_tx);

        assert_eq!(deny_rx.try_recv().unwrap(), PermissionDecision::Deny);
        assert!(deny_state.pending_permission.is_none());
        assert_eq!(deny_state.phase, Phase::Busy);
    }

    #[test]
    fn on_key_answers_pending_permission_with_enter_or_escape() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut allow_state = AppState::new();
        allow_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: allow_tx,
        }));

        allow_state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionDecision::Allow);

        let (deny_tx, mut deny_rx) = oneshot::channel();
        let mut deny_state = AppState::new();
        deny_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            responder: deny_tx,
        }));

        deny_state.on_key(key(KeyCode::Esc), &input_tx);

        assert_eq!(deny_rx.try_recv().unwrap(), PermissionDecision::Deny);
    }

    #[test]
    fn record_usage_accumulates_output_tokens_and_computes_rate() {
        let mut state = AppState::new();

        state.record_usage(
            Usage {
                input_tokens: 10,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        assert_eq!(state.output_tokens_this_turn(), 120);
        assert_eq!(state.last_rate_tps(), Some(60.0));

        state.record_usage(
            Usage {
                input_tokens: 5,
                output_tokens: 60,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 180);
        assert_eq!(state.last_rate_tps(), Some(60.0));
    }

    #[test]
    fn record_usage_zero_elapsed_has_no_rate_without_panic() {
        let mut state = AppState::new();

        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 50,
            },
            Duration::ZERO,
        );

        assert_eq!(state.output_tokens_this_turn(), 50);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn turn_complete_resets_output_token_accumulation() {
        let mut state = AppState::new();
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 40);
        assert_eq!(state.last_rate_tps(), Some(40.0));

        state.apply(AgentEvent::TurnComplete);

        assert_eq!(state.output_tokens_this_turn(), 0);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn new_prompt_resets_output_token_accumulation() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 40);
        state.input = "hello".to_string();

        state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(state.output_tokens_this_turn(), 0);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn estimate_tokens_from_chars_uses_quarter_char_ratio() {
        assert_eq!(estimate_tokens_from_chars(400), 100);
    }

    #[test]
    fn estimate_streaming_rate_tps_uses_chars_per_token_ratio() {
        let rate = estimate_streaming_rate_tps(400, Duration::from_secs(2)).unwrap();
        assert!((rate - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_streaming_rate_tps_zero_elapsed_returns_none() {
        assert_eq!(estimate_streaming_rate_tps(100, Duration::ZERO), None);
    }

    #[test]
    fn record_streaming_chars_sets_approximate_rate_before_real_usage_corrects() {
        let mut state = AppState::new();
        state.record_streaming_chars(400, Duration::from_secs(2));
        assert!(state.last_rate_is_approximate());
        assert_eq!(state.last_rate_tps(), Some(50.0));

        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        assert!(!state.last_rate_is_approximate());
        assert_eq!(state.last_rate_tps(), Some(60.0));
        assert_eq!(state.output_tokens_this_turn(), 120);
    }
}
