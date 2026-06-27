use crate::permission::PermissionDecision;
use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Busy,
    WaitingForPermission,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptBlock {
    User(String),
    Assistant(String),
    Error(String),
}

pub struct AppState {
    pub transcript: Vec<TranscriptBlock>,
    pub input: String,
    pub phase: Phase,
    pub pending_permission: Option<PermissionRequest>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            input: String::new(),
            phase: Phase::Ready,
            pending_permission: None,
        }
    }

    pub fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                self.phase = Phase::Busy;
                match self.transcript.last_mut() {
                    Some(TranscriptBlock::Assistant(current)) => current.push_str(&text),
                    _ => self.transcript.push(TranscriptBlock::Assistant(text)),
                }
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

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, Phase, TranscriptBlock};
    use crate::permission::PermissionDecision;
    use crate::tui::channel::{AgentEvent, PermissionRequest, UserInput};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;
    use tokio::sync::{mpsc, oneshot};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
