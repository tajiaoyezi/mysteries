//! 主输入态 ↑↓ 历史导航纯函数 reducer(§3 TUI)。

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InputHistoryState {
    pub input_history: Vec<String>,
    /// `None` = 草稿态; `Some(i)` = 正在浏览 `input_history[i]`。
    pub history_cursor: Option<usize>,
    /// 进入历史前暂存的未提交草稿。
    pub draft: String,
    /// 当前输入框文本。
    pub input: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputHistoryAction {
    PushSubmitted(String),
    HistoryUp,
    HistoryDown,
    InsertChar(char),
    Backspace,
}

pub fn reduce_input_history(
    state: &InputHistoryState,
    action: InputHistoryAction,
) -> InputHistoryState {
    let mut next = state.clone();
    match action {
        InputHistoryAction::PushSubmitted(text) => {
            let text = text.trim().to_string();
            if !text.is_empty() && next.input_history.last() != Some(&text) {
                next.input_history.push(text);
            }
            next.input.clear();
            next.history_cursor = None;
            next.draft.clear();
        }
        InputHistoryAction::HistoryUp => {
            if next.input_history.is_empty() {
                return next;
            }
            match next.history_cursor {
                None => {
                    if !next.input.is_empty() {
                        next.draft = next.input.clone();
                    }
                    let index = next.input_history.len() - 1;
                    next.history_cursor = Some(index);
                    next.input = next.input_history[index].clone();
                }
                Some(index) if index > 0 => {
                    let new_index = index - 1;
                    next.history_cursor = Some(new_index);
                    next.input = next.input_history[new_index].clone();
                }
                Some(_) => {}
            }
        }
        InputHistoryAction::HistoryDown => match next.history_cursor {
            None => {}
            Some(index) if index + 1 < next.input_history.len() => {
                let new_index = index + 1;
                next.history_cursor = Some(new_index);
                next.input = next.input_history[new_index].clone();
            }
            Some(_) => {
                next.history_cursor = None;
                next.input = next.draft.clone();
            }
        },
        InputHistoryAction::InsertChar(ch) => {
            if next.history_cursor.is_some() {
                next.history_cursor = None;
            }
            next.input.push(ch);
        }
        InputHistoryAction::Backspace => {
            if next.history_cursor.is_some() {
                next.history_cursor = None;
            }
            next.input.pop();
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::{reduce_input_history, InputHistoryAction, InputHistoryState};

    fn push(state: &InputHistoryState, text: &str) -> InputHistoryState {
        reduce_input_history(state, InputHistoryAction::PushSubmitted(text.to_string()))
    }

    fn up(state: &InputHistoryState) -> InputHistoryState {
        reduce_input_history(state, InputHistoryAction::HistoryUp)
    }

    fn down(state: &InputHistoryState) -> InputHistoryState {
        reduce_input_history(state, InputHistoryAction::HistoryDown)
    }

    fn insert(state: &InputHistoryState, ch: char) -> InputHistoryState {
        reduce_input_history(state, InputHistoryAction::InsertChar(ch))
    }

    fn backspace(state: &InputHistoryState) -> InputHistoryState {
        reduce_input_history(state, InputHistoryAction::Backspace)
    }

    #[test]
    fn history_up_walks_submitted_entries_newest_first() {
        let state = push(&push(&InputHistoryState::default(), "a"), "b");

        let state = up(&state);
        assert_eq!(state.input, "b");
        assert_eq!(state.history_cursor, Some(1));

        let state = up(&state);
        assert_eq!(state.input, "a");
        assert_eq!(state.history_cursor, Some(0));
    }

    #[test]
    fn history_down_advances_forward() {
        let state = up(&up(&push(&push(&InputHistoryState::default(), "a"), "b")));

        let state = down(&state);
        assert_eq!(state.input, "b");
        assert_eq!(state.history_cursor, Some(1));
    }

    #[test]
    fn history_down_past_latest_restores_draft() {
        let mut state = push(&InputHistoryState::default(), "a");
        state.input = "dr".to_string();

        let state = up(&state);
        assert_eq!(state.draft, "dr");
        assert_eq!(state.input, "a");

        let state = down(&state);
        assert_eq!(state.input, "dr");
        assert_eq!(state.history_cursor, None);
    }

    #[test]
    fn typing_char_exits_history_and_appends_to_current_text() {
        let state = up(&up(&push(&push(&InputHistoryState::default(), "a"), "b")));

        let state = insert(&state, 'z');
        assert_eq!(state.history_cursor, None);
        assert_eq!(state.input, "az");

        let state = up(&state);
        assert_eq!(state.input, "b");
        assert_eq!(state.history_cursor, Some(1));
    }

    #[test]
    fn backspace_exits_history_and_edits_current_text() {
        let state = up(&push(&InputHistoryState::default(), "ab"));

        let state = backspace(&state);
        assert_eq!(state.history_cursor, None);
        assert_eq!(state.input, "a");
    }

    #[test]
    fn consecutive_duplicate_submits_deduplicate_history() {
        let state = push(&push(&InputHistoryState::default(), "x"), "x");

        assert_eq!(state.input_history, vec!["x"]);
        assert_eq!(state.input, "");
        assert_eq!(state.history_cursor, None);
    }
}
