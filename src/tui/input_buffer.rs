use crate::tui::width::{char_width, display_width};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputBufferState {
    pub text: String,
    pub cursor: usize,
    pub input_history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub draft: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputBufferAction {
    PushSubmitted(String),
    SetText(String),
    InsertChar(char),
    InsertNewline,
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveLineStart,
    MoveLineEnd,
    Up,
    Down,
}

impl InputBufferState {
    pub fn text(&self) -> &str {
        &self.text
    }

    fn exit_history(&mut self) {
        if self.history_cursor.is_some() {
            self.history_cursor = None;
        }
    }
}

pub fn reduce_input_buffer(
    state: &InputBufferState,
    action: InputBufferAction,
) -> InputBufferState {
    let mut next = state.clone();
    match action {
        InputBufferAction::PushSubmitted(text) => {
            let text = text.trim().to_string();
            if !text.is_empty() && next.input_history.last() != Some(&text) {
                next.input_history.push(text);
            }
            next.text.clear();
            next.cursor = 0;
            next.history_cursor = None;
            next.draft.clear();
        }
        InputBufferAction::SetText(text) => {
            next.text = text;
            next.cursor = next.text.len();
            next.history_cursor = None;
        }
        InputBufferAction::InsertChar(ch) => {
            next.exit_history();
            next.text.insert(next.cursor, ch);
            next.cursor += ch.len_utf8();
        }
        InputBufferAction::InsertNewline => {
            next.exit_history();
            next.text.insert(next.cursor, '\n');
            next.cursor += 1;
        }
        InputBufferAction::Backspace => {
            next.exit_history();
            if let Some(previous) = previous_char_boundary(&next.text, next.cursor) {
                next.text.drain(previous..next.cursor);
                next.cursor = previous;
            }
        }
        InputBufferAction::Delete => {
            next.exit_history();
            if let Some(after) = next_char_boundary(&next.text, next.cursor) {
                next.text.drain(next.cursor..after);
            }
        }
        InputBufferAction::MoveLeft => {
            if let Some(previous) = previous_char_boundary(&next.text, next.cursor) {
                next.cursor = previous;
            }
        }
        InputBufferAction::MoveRight => {
            if let Some(after) = next_char_boundary(&next.text, next.cursor) {
                next.cursor = after;
            }
        }
        InputBufferAction::MoveLineStart => {
            next.cursor = line_start(&next.text, next.cursor);
        }
        InputBufferAction::MoveLineEnd => {
            next.cursor = line_end(&next.text, next.cursor);
        }
        InputBufferAction::Up => {
            let range = current_line_range(&next.text, next.cursor);
            if let Some(previous) = previous_line_range(&next.text, range.start) {
                let target_column = display_width(&next.text[range.start..next.cursor]);
                next.cursor = cursor_for_display_column(&next.text, previous, target_column);
            } else {
                history_up(&mut next);
            }
        }
        InputBufferAction::Down => {
            let range = current_line_range(&next.text, next.cursor);
            if let Some(following) = next_line_range(&next.text, range.end) {
                let target_column = display_width(&next.text[range.start..next.cursor]);
                next.cursor = cursor_for_display_column(&next.text, following, target_column);
            } else {
                history_down(&mut next);
            }
        }
    }
    next
}

#[derive(Clone, Copy)]
struct LineRange {
    start: usize,
    end: usize,
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor].char_indices().last().map(|(index, _)| index)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn line_start(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn line_end(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len())
}

fn current_line_range(text: &str, cursor: usize) -> LineRange {
    LineRange {
        start: line_start(text, cursor),
        end: line_end(text, cursor),
    }
}

fn previous_line_range(text: &str, current_start: usize) -> Option<LineRange> {
    if current_start == 0 {
        return None;
    }
    let end = current_start - 1;
    Some(LineRange {
        start: line_start(text, end),
        end,
    })
}

fn next_line_range(text: &str, current_end: usize) -> Option<LineRange> {
    if current_end == text.len() {
        return None;
    }
    let start = current_end + 1;
    Some(LineRange {
        start,
        end: line_end(text, start),
    })
}

fn cursor_for_display_column(text: &str, range: LineRange, target_column: usize) -> usize {
    let mut column = 0;
    let mut cursor = range.start;
    for (offset, ch) in text[range.start..range.end].char_indices() {
        let width = char_width(ch);
        if column + width > target_column {
            break;
        }
        column += width;
        cursor = range.start + offset + ch.len_utf8();
    }
    cursor
}

fn history_up(next: &mut InputBufferState) {
    if next.input_history.is_empty() {
        return;
    }
    match next.history_cursor {
        None => {
            next.draft = next.text.clone();
            let index = next.input_history.len() - 1;
            next.history_cursor = Some(index);
            next.text = next.input_history[index].clone();
            next.cursor = next.text.len();
        }
        Some(index) if index > 0 => {
            let index = index - 1;
            next.history_cursor = Some(index);
            next.text = next.input_history[index].clone();
            next.cursor = next.text.len();
        }
        Some(_) => {}
    }
}

fn history_down(next: &mut InputBufferState) {
    match next.history_cursor {
        None => {}
        Some(index) if index + 1 < next.input_history.len() => {
            let index = index + 1;
            next.history_cursor = Some(index);
            next.text = next.input_history[index].clone();
            next.cursor = next.text.len();
        }
        Some(_) => {
            next.history_cursor = None;
            next.text = next.draft.clone();
            next.cursor = next.text.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{reduce_input_buffer, InputBufferAction, InputBufferState};

    fn reduce(state: &InputBufferState, action: InputBufferAction) -> InputBufferState {
        let next = reduce_input_buffer(state, action);
        assert!(
            next.text.is_char_boundary(next.cursor),
            "cursor {} must be a char boundary in {:?}",
            next.cursor,
            next.text
        );
        next
    }

    fn buffer_state(text: &str, cursor: usize) -> InputBufferState {
        assert!(text.is_char_boundary(cursor));
        InputBufferState {
            text: text.to_string(),
            cursor,
            ..InputBufferState::default()
        }
    }

    fn with_history(entries: &[&str], text: &str, cursor: usize) -> InputBufferState {
        InputBufferState {
            text: text.to_string(),
            cursor,
            input_history: entries.iter().map(|entry| (*entry).to_string()).collect(),
            ..InputBufferState::default()
        }
    }

    #[test]
    fn insert_newline_backspace_delete_and_set_text_update_text_and_cursor() {
        let state = reduce(&InputBufferState::default(), InputBufferAction::Backspace);
        assert_eq!(state.text(), "");
        assert_eq!(state.cursor, 0);

        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertChar('你'),
        );
        assert_eq!(state.text(), "你");
        assert_eq!(state.cursor, "你".len());

        let state = reduce(&state, InputBufferAction::InsertNewline);
        assert_eq!(state.text(), "你\n");
        assert_eq!(state.cursor, "你\n".len());

        let state = reduce(&state, InputBufferAction::InsertChar('好'));
        assert_eq!(state.text(), "你\n好");
        assert_eq!(state.cursor, "你\n好".len());

        let state = reduce(&state, InputBufferAction::MoveLeft);
        let state = reduce(&state, InputBufferAction::Backspace);
        assert_eq!(state.text(), "你好");
        assert_eq!(state.cursor, "你".len());

        let state = reduce(&state, InputBufferAction::Delete);
        assert_eq!(state.text(), "你");
        assert_eq!(state.cursor, "你".len());

        let state = reduce(&state, InputBufferAction::SetText("ab\n你".to_string()));
        assert_eq!(state.text(), "ab\n你");
        assert_eq!(state.cursor, "ab\n你".len());

        let state = reduce(&state, InputBufferAction::Delete);
        assert_eq!(state.text(), "ab\n你");
        assert_eq!(state.cursor, "ab\n你".len());
    }

    #[test]
    fn left_right_and_home_end_move_by_char_and_current_line() {
        let state = reduce(
            &buffer_state("a你b", "a你b".len()),
            InputBufferAction::MoveLeft,
        );
        assert_eq!(
            state.cursor,
            "a你".len(),
            "MoveLeft crosses one trailing ASCII char"
        );

        let state = reduce(&state, InputBufferAction::MoveLeft);
        assert_eq!(state.cursor, "a".len());

        let state = reduce(&state, InputBufferAction::MoveRight);
        assert_eq!(
            state.cursor,
            "a你".len(),
            "MoveRight crosses one full CJK char"
        );

        let state = reduce(
            &buffer_state("ab\ncd你", "ab\nc".len()),
            InputBufferAction::MoveLineStart,
        );
        assert_eq!(state.cursor, "ab\n".len());

        let state = reduce(&state, InputBufferAction::MoveLineEnd);
        assert_eq!(state.cursor, "ab\ncd你".len());
    }

    #[test]
    fn up_down_move_inside_multiline_before_using_history() {
        let state = with_history(&["old"], "aa\nbb", "aa\nb".len());

        let moved = reduce(&state, InputBufferAction::Up);
        assert_eq!(moved.text(), "aa\nbb");
        assert_eq!(
            moved.cursor,
            "a".len(),
            "first Up moves to previous logical line"
        );
        assert_eq!(moved.history_cursor, None);

        let recalled = reduce(&moved, InputBufferAction::Up);
        assert_eq!(recalled.text(), "old");
        assert_eq!(recalled.cursor, "old".len());
        assert_eq!(recalled.history_cursor, Some(0));
        assert_eq!(recalled.draft, "aa\nbb");

        let restored = reduce(&recalled, InputBufferAction::Down);
        assert_eq!(restored.text(), "aa\nbb");
        assert_eq!(restored.cursor, "aa\nbb".len());
        assert_eq!(restored.history_cursor, None);

        let unchanged = reduce(
            &buffer_state("aa\nbb", "aa\nbb".len()),
            InputBufferAction::Down,
        );
        assert_eq!(unchanged.text(), "aa\nbb");
        assert_eq!(unchanged.cursor, "aa\nbb".len());
        assert_eq!(unchanged.history_cursor, None);
    }

    #[test]
    fn single_line_up_uses_history_and_typing_exits_history_at_cursor() {
        let state = with_history(&["a", "b"], "", 0);

        let recalled = reduce(&state, InputBufferAction::Up);
        assert_eq!(recalled.text(), "b");
        assert_eq!(recalled.cursor, "b".len());
        assert_eq!(recalled.history_cursor, Some(1));

        let recalled = reduce(&recalled, InputBufferAction::Up);
        assert_eq!(recalled.text(), "a");
        assert_eq!(recalled.history_cursor, Some(0));

        let edited = reduce(&recalled, InputBufferAction::InsertChar('z'));
        assert_eq!(edited.text(), "az");
        assert_eq!(edited.cursor, "az".len());
        assert_eq!(edited.history_cursor, None);

        let recalled_latest = reduce(&edited, InputBufferAction::Up);
        assert_eq!(recalled_latest.text(), "b");
        assert_eq!(recalled_latest.history_cursor, Some(1));
    }

    #[test]
    fn history_down_after_deleted_draft_restores_empty_buffer() {
        let mut state = with_history(&["h1"], "", 0);
        for ch in ['h', 'e', 'l', 'l', 'o'] {
            state = reduce(&state, InputBufferAction::InsertChar(ch));
        }

        state = reduce(&state, InputBufferAction::Up);
        state = reduce(&state, InputBufferAction::Down);
        for _ in 0..5 {
            state = reduce(&state, InputBufferAction::Backspace);
        }
        state = reduce(&state, InputBufferAction::Up);
        state = reduce(&state, InputBufferAction::Down);

        assert_eq!(state.text(), "");
        assert_eq!(state.cursor, 0);
        assert_eq!(state.history_cursor, None);
    }

    #[test]
    fn cjk_vertical_alignment_uses_display_width_and_ties_left() {
        let down = reduce(
            &buffer_state("ab你\nx你好", "ab你".len()),
            InputBufferAction::Down,
        );
        assert_eq!(
            down.cursor,
            "ab你\nx你".len(),
            "target display col 4 lands inside 好, so cursor chooses the boundary before 好"
        );

        let down = reduce(
            &buffer_state("x你好\nab你", "x你".len()),
            InputBufferAction::Down,
        );
        assert_eq!(
            down.cursor,
            "x你好\nab".len(),
            "target display col 3 lands inside 你, so cursor chooses the boundary before 你"
        );
    }

    #[test]
    fn push_submitted_trims_dedupes_clears_text_and_resets_history_cursor() {
        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::PushSubmitted(" x ".to_string()),
        );
        assert_eq!(state.input_history, vec!["x".to_string()]);
        assert_eq!(state.text(), "");
        assert_eq!(state.cursor, 0);
        assert_eq!(state.history_cursor, None);

        let state = reduce(&state, InputBufferAction::PushSubmitted("x".to_string()));
        assert_eq!(state.input_history, vec!["x".to_string()]);
    }
}
