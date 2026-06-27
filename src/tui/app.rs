use crate::agent::AgentStatus;
use crate::permission::PermissionDecision;
use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;
use tokio::sync::mpsc;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptBlock {
    User(String),
    Assistant(String),
    Error(String),
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

pub struct AppState {
    pub transcript: Vec<TranscriptBlock>,
    pub tool_cards: Vec<ToolCard>,
    pub input: String,
    pub phase: Phase,
    pub pending_permission: Option<PermissionRequest>,
    pub scroll_offset: usize,
    pub spinner_frame: usize,
    follows_bottom: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            tool_cards: Vec::new(),
            input: String::new(),
            phase: Phase::Ready,
            pending_permission: None,
            scroll_offset: 0,
            spinner_frame: 0,
            follows_bottom: true,
        }
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

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner_glyph(&self) -> &'static str {
        spinner_glyph(self.spinner_frame, true)
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
                self.tool_cards.push(ToolCard {
                    id,
                    name,
                    args,
                    readonly,
                    status: ToolCardStatus::Running,
                    output: None,
                    truncated: false,
                });
            }
            AgentEvent::ToolCallFinished { id, outcome } => {
                let status = if outcome.is_error {
                    ToolCardStatus::Error
                } else {
                    ToolCardStatus::Done
                };
                let card = self.tool_cards.iter_mut().find(|card| card.id == id);

                match card {
                    Some(card) => {
                        card.status = status;
                        card.output = Some(outcome.content);
                        card.truncated = outcome.truncated;
                    }
                    None => {
                        self.tool_cards.push(ToolCard {
                            id: id.clone(),
                            name: id,
                            args: Value::Null,
                            readonly: false,
                            status,
                            output: Some(outcome.content),
                            truncated: outcome.truncated,
                        });
                    }
                }
            }
            AgentEvent::StatusChanged(status) => {
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
                self.phase = Phase::Ready;
            }
            AgentEvent::Error(message) => {
                self.pending_permission = None;
                self.phase = Phase::Ready;
                self.transcript.push(TranscriptBlock::Error(message));
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, input_tx: &mpsc::UnboundedSender<UserInput>) {
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

        match key.code {
            KeyCode::Char(ch) => self.input.push(ch),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Enter => {
                let prompt = self.input.trim().to_string();
                if prompt.is_empty() {
                    return;
                }
                self.input.clear();
                self.phase = Phase::Busy;
                self.transcript.push(TranscriptBlock::User(prompt.clone()));
                let _ = input_tx.send(UserInput::Prompt(prompt));
            }
            _ => {}
        }
    }

    fn answer_pending_permission(&mut self, decision: PermissionDecision) {
        if let Some(request) = self.pending_permission.take() {
            let _ = request.responder.send(decision);
            self.phase = Phase::Busy;
        }
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
        compute_diff, AppState, DiffKind, DiffLine, Phase, ToolCard, ToolCardStatus,
        TranscriptBlock,
    };
    use crate::agent::AgentStatus;
    use crate::permission::PermissionDecision;
    use crate::tool::ToolOutcome;
    use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;
    use tokio::sync::{mpsc, oneshot};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn diff_line(kind: DiffKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
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
    fn apply_tool_call_started_adds_running_tool_card() {
        let mut state = AppState::new();

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
        });

        assert_eq!(
            state.tool_cards,
            vec![ToolCard {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                args: json!({ "path": "note.txt" }),
                readonly: true,
                status: ToolCardStatus::Running,
                output: None,
                truncated: false,
            }]
        );
    }

    #[test]
    fn apply_tool_call_finished_updates_card_to_done_or_error() {
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
            },
        });

        assert_eq!(state.tool_cards[0].status, ToolCardStatus::Done);
        assert_eq!(
            state.tool_cards[0].output.as_deref(),
            Some("wrote note.txt")
        );

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
            },
        });

        assert_eq!(state.tool_cards[1].status, ToolCardStatus::Error);
        assert_eq!(
            state.tool_cards[1].output.as_deref(),
            Some("command failed: permission denied")
        );
        assert!(state.tool_cards[1].truncated);
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
}
