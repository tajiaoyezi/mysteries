use std::collections::BTreeMap;

use crate::tui::width::{char_width, display_width};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastedChunk {
    pub seq: u32,
    pub text: String,
    pub line_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputBufferState {
    pub text: String,
    pub cursor: usize,
    pub input_history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub draft: String,
    pub pasted: BTreeMap<char, PastedChunk>,
    pub next_paste_seq: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputBufferAction {
    PushSubmitted(String),
    SetText(String),
    InsertChar(char),
    InsertStr(String),
    InsertPasteFold(String),
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

    pub fn expand_folds(&self) -> String {
        let mut out = String::with_capacity(self.text.len());
        for ch in self.text.chars() {
            match self.pasted.get(&ch) {
                Some(chunk) => out.push_str(&chunk.text),
                None => out.push(ch),
            }
        }
        out
    }

    pub fn prune_pasted(&mut self) {
        let text = &self.text;
        let draft = &self.draft;
        self.pasted
            .retain(|sentinel, _| text.contains(*sentinel) || draft.contains(*sentinel));
        if self.pasted.is_empty() {
            self.next_paste_seq = 0;
        }
    }

    fn exit_history(&mut self) {
        if self.history_cursor.is_some() {
            self.history_cursor = None;
            self.draft.clear();
            self.prune_pasted();
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
            next.pasted.clear();
            next.next_paste_seq = 0;
        }
        InputBufferAction::SetText(text) => {
            next.text = text;
            next.cursor = next.text.len();
            next.history_cursor = None;
            next.draft.clear();
            next.prune_pasted();
        }
        InputBufferAction::InsertChar(ch) => {
            next.exit_history();
            next.text.insert(next.cursor, ch);
            next.cursor += ch.len_utf8();
        }
        InputBufferAction::InsertStr(s) => {
            next.exit_history();
            next.text.insert_str(next.cursor, &s);
            next.cursor += s.len();
        }
        InputBufferAction::InsertPasteFold(s) => {
            next.exit_history();
            let seq = next.next_paste_seq;
            let sentinel = char::from_u32(0xE000 + seq).expect("valid PUA sentinel");
            let line_count = s.split('\n').count();
            next.text.insert(next.cursor, sentinel);
            next.cursor += sentinel.len_utf8();
            next.pasted.insert(
                sentinel,
                PastedChunk {
                    seq,
                    text: s,
                    line_count,
                },
            );
            next.next_paste_seq += 1;
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
            next.prune_pasted();
        }
        InputBufferAction::Delete => {
            next.exit_history();
            if let Some(after) = next_char_boundary(&next.text, next.cursor) {
                next.text.drain(next.cursor..after);
            }
            next.prune_pasted();
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
    next.prune_pasted();
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
            next.draft.clear();
        }
    }
    next.prune_pasted();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{reduce_input_buffer, InputBufferAction, InputBufferState, PastedChunk};

    fn paste_sentinel(seq: u32) -> char {
        char::from_u32(0xE000 + seq).expect("valid PUA sentinel")
    }

    fn paste_sentinels_in_text(text: &str) -> Vec<char> {
        text.chars()
            .filter(|c| {
                let code = *c as u32;
                (0xE000..=0xF8FF).contains(&code)
            })
            .collect()
    }

    fn pasted_chunk(seq: u32, text: &str) -> PastedChunk {
        PastedChunk {
            seq,
            text: text.to_string(),
            line_count: text.split('\n').count(),
        }
    }

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
    fn insert_str_empty_keeps_text_and_cursor_and_resets_history_cursor() {
        let state = InputBufferState {
            text: "old".to_string(),
            cursor: "old".len(),
            input_history: vec!["old".to_string()],
            history_cursor: Some(0),
            ..InputBufferState::default()
        };

        let next = reduce(&state, InputBufferAction::InsertStr(String::new()));
        assert_eq!(next.text(), "old");
        assert_eq!(next.cursor, "old".len());
        assert_eq!(next.history_cursor, None);
    }

    #[test]
    fn insert_str_with_cjk_moves_cursor_to_end_of_inserted_text() {
        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertStr("你好a".to_string()),
        );
        assert_eq!(state.text(), "你好a");
        assert_eq!(state.cursor, "你好a".len());
    }

    #[test]
    fn insert_str_at_middle_of_text_inserts_at_cursor() {
        let state = reduce(
            &buffer_state("a你b", "a".len()),
            InputBufferAction::InsertStr("好x".to_string()),
        );
        assert_eq!(state.text(), "a好x你b");
        assert_eq!(state.cursor, "a好x".len());
    }

    #[test]
    fn insert_str_equals_folding_insert_char() {
        let s = "粘贴 line\nab你好";
        let initial = buffer_state("头尾", "头".len());

        let via_str = reduce(&initial, InputBufferAction::InsertStr(s.to_string()));
        let via_chars = s.chars().fold(initial, |state, ch| {
            reduce(&state, InputBufferAction::InsertChar(ch))
        });

        assert_eq!(via_str.text(), via_chars.text());
        assert_eq!(via_str.cursor, via_chars.cursor);
    }

    #[test]
    fn insert_str_exits_history_like_insert_char() {
        let state = with_history(&["h1"], "", 0);
        let recalled = reduce(&state, InputBufferAction::Up);
        assert_eq!(recalled.history_cursor, Some(0));

        let edited = reduce(&recalled, InputBufferAction::InsertStr("xy".to_string()));
        assert_eq!(edited.text(), "h1xy");
        assert_eq!(edited.cursor, "h1xy".len());
        assert_eq!(edited.history_cursor, None);
    }

    #[test]
    fn insert_paste_fold_inserts_sentinel_at_cursor_and_records_chunk() {
        let pasted_text = "line1\nline2\nline3";
        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertPasteFold(pasted_text.to_string()),
        );

        let sentinels = paste_sentinels_in_text(&state.text);
        assert_eq!(sentinels.len(), 1, "text must contain exactly one sentinel");
        let sentinel = sentinels[0];
        assert_eq!(sentinel, paste_sentinel(0));

        assert_eq!(
            state.cursor,
            sentinel.len_utf8(),
            "cursor lands immediately after the sentinel"
        );

        assert_eq!(state.pasted.len(), 1);
        let chunk = state.pasted.get(&sentinel).expect("sentinel mapped in pasted");
        assert_eq!(
            chunk,
            &PastedChunk {
                seq: 0,
                text: pasted_text.to_string(),
                line_count: pasted_text.split('\n').count(),
            }
        );
        assert_eq!(state.next_paste_seq, 1);
    }

    #[test]
    fn insert_paste_fold_twice_assigns_distinct_sentinels_and_seqs() {
        let first = "a\nb";
        let second = "c\nd\ne";

        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertPasteFold(first.to_string()),
        );
        let state = reduce(
            &state,
            InputBufferAction::InsertPasteFold(second.to_string()),
        );

        let sentinels = paste_sentinels_in_text(&state.text);
        assert_eq!(sentinels.len(), 2);
        assert_eq!(sentinels[0], paste_sentinel(0));
        assert_eq!(sentinels[1], paste_sentinel(1));

        assert_eq!(state.pasted.len(), 2);
        assert_eq!(state.pasted.get(&paste_sentinel(0)).unwrap().seq, 0);
        assert_eq!(state.pasted.get(&paste_sentinel(1)).unwrap().seq, 1);
        assert_eq!(state.next_paste_seq, 2);
    }

    #[test]
    fn insert_paste_fold_in_middle_of_existing_text() {
        let pasted_text = "mid\nline";
        let initial = buffer_state("头尾", "头".len());

        let state = reduce(
            &initial,
            InputBufferAction::InsertPasteFold(pasted_text.to_string()),
        );

        let sentinel = paste_sentinel(0);
        assert_eq!(state.text, format!("头{sentinel}尾"));
        assert_eq!(
            state.cursor,
            "头".len() + sentinel.len_utf8(),
            "cursor sits between sentinel and trailing text"
        );
        assert_eq!(state.pasted.len(), 1);
        assert_eq!(state.pasted.get(&sentinel).unwrap().text, pasted_text);
    }

    // --- Task 1.2 RED: expand_folds / prune_pasted (A 类 panic-RED) ---

    #[test]
    fn expand_folds_expands_mixed_text_and_single_fold() {
        let state = reduce(&InputBufferState::default(), InputBufferAction::InsertChar('a'));
        let state = reduce(
            &state,
            InputBufferAction::InsertPasteFold("X\nY".to_string()),
        );
        let state = reduce(&state, InputBufferAction::InsertChar('b'));

        assert_eq!(state.expand_folds(), "aX\nYb");
    }

    #[test]
    fn expand_folds_preserves_order_with_multiple_folds_and_text_between() {
        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertPasteFold("A".to_string()),
        );
        let state = reduce(&state, InputBufferAction::InsertChar('中'));
        let state = reduce(
            &state,
            InputBufferAction::InsertPasteFold("B".to_string()),
        );

        assert_eq!(state.expand_folds(), "A中B");
    }

    #[test]
    fn prune_pasted_drops_orphan_entries_not_present_in_text() {
        let s0 = paste_sentinel(0);
        let s1 = paste_sentinel(1);
        let mut state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            pasted: BTreeMap::from([
                (
                    s0,
                    PastedChunk {
                        seq: 0,
                        text: "kept".to_string(),
                        line_count: 1,
                    },
                ),
                (
                    s1,
                    PastedChunk {
                        seq: 1,
                        text: "orphan".to_string(),
                        line_count: 1,
                    },
                ),
            ]),
            next_paste_seq: 2,
            ..InputBufferState::default()
        };

        state.prune_pasted();

        assert_eq!(state.pasted.len(), 1);
        assert!(state.pasted.contains_key(&s0));
        assert!(!state.pasted.contains_key(&s1));
    }

    #[test]
    fn prune_pasted_keeps_draft_only_sentinel_and_drops_true_orphan() {
        let s0 = paste_sentinel(0);
        let s1 = paste_sentinel(1);
        let mut state = InputBufferState {
            draft: s0.to_string(),
            pasted: BTreeMap::from([
                (s0, pasted_chunk(0, "draft only")),
                (s1, pasted_chunk(1, "orphan")),
            ]),
            next_paste_seq: 2,
            ..InputBufferState::default()
        };

        state.prune_pasted();

        assert_eq!(state.pasted.len(), 1);
        assert!(state.pasted.contains_key(&s0));
        assert!(!state.pasted.contains_key(&s1));
    }

    #[test]
    fn history_up_prunes_orphans_but_keeps_draft_referenced_fold() {
        let s0 = paste_sentinel(0);
        let s1 = paste_sentinel(1);
        let state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            input_history: vec!["history".to_string()],
            pasted: BTreeMap::from([
                (s0, pasted_chunk(0, "draft fold")),
                (s1, pasted_chunk(1, "orphan")),
            ]),
            next_paste_seq: 2,
            ..InputBufferState::default()
        };

        let state = reduce(&state, InputBufferAction::Up);

        assert_eq!(state.text(), "history");
        assert_eq!(state.draft, s0.to_string());
        assert_eq!(state.pasted.len(), 1);
        assert!(state.pasted.contains_key(&s0));
        assert!(!state.pasted.contains_key(&s1));
    }

    #[test]
    fn history_up_down_roundtrip_preserves_fold_chunk() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            input_history: vec!["history".to_string()],
            pasted: BTreeMap::from([(s0, pasted_chunk(0, "fold text"))]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let recalled = reduce(&state, InputBufferAction::Up);
        let restored = reduce(&recalled, InputBufferAction::Down);

        assert_eq!(restored.text(), s0.to_string());
        assert_eq!(restored.expand_folds(), "fold text");
        assert!(restored.pasted.contains_key(&s0));
    }

    #[test]
    fn history_down_restore_consumes_draft_but_keeps_restored_fold() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            input_history: vec!["history".to_string()],
            pasted: BTreeMap::from([(s0, pasted_chunk(0, "fold text"))]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let recalled = reduce(&state, InputBufferAction::Up);
        let restored = reduce(&recalled, InputBufferAction::Down);

        assert_eq!(restored.text(), s0.to_string());
        assert_eq!(restored.draft, "");
        assert!(restored.pasted.contains_key(&s0));
    }

    #[test]
    fn restored_fold_deleted_by_backspace_clears_pasted_and_resets_sequence() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            input_history: vec!["history".to_string()],
            pasted: BTreeMap::from([(s0, pasted_chunk(0, "fold text"))]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let recalled = reduce(&state, InputBufferAction::Up);
        let restored = reduce(&recalled, InputBufferAction::Down);
        let deleted = reduce(&restored, InputBufferAction::Backspace);

        assert!(deleted.pasted.is_empty());
        assert_eq!(deleted.next_paste_seq, 0);
    }

    #[test]
    fn typing_after_history_recall_discards_draft_and_prunes_draft_only_fold() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            text: s0.to_string(),
            cursor: s0.len_utf8(),
            input_history: vec!["history".to_string()],
            pasted: BTreeMap::from([(s0, pasted_chunk(0, "draft fold"))]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let recalled = reduce(&state, InputBufferAction::Up);
        let edited = reduce(&recalled, InputBufferAction::InsertChar('x'));

        assert_eq!(edited.history_cursor, None);
        assert_eq!(edited.draft, "");
        assert!(edited.pasted.is_empty());
        assert_eq!(edited.next_paste_seq, 0);
    }

    #[test]
    fn set_text_discards_draft_prunes_pasted_and_resets_sequence_when_empty() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            draft: s0.to_string(),
            pasted: BTreeMap::from([(s0, pasted_chunk(0, "draft fold"))]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let state = reduce(&state, InputBufferAction::SetText("plain".to_string()));

        assert_eq!(state.text(), "plain");
        assert_eq!(state.draft, "");
        assert!(state.pasted.is_empty());
        assert_eq!(state.next_paste_seq, 0);
    }

    #[test]
    fn deleting_only_fold_resets_sequence_and_next_fold_reuses_first_sentinel() {
        let state = reduce(
            &InputBufferState::default(),
            InputBufferAction::InsertPasteFold("first".to_string()),
        );

        let deleted = reduce(&state, InputBufferAction::Backspace);
        assert!(deleted.pasted.is_empty());
        assert_eq!(deleted.next_paste_seq, 0);

        let inserted = reduce(
            &deleted,
            InputBufferAction::InsertPasteFold("second".to_string()),
        );

        assert_eq!(inserted.text(), paste_sentinel(0).to_string());
        assert_eq!(inserted.pasted.get(&paste_sentinel(0)).unwrap().seq, 0);
    }

    // --- Task 1.2 RED: PushSubmitted / Backspace wiring (B 类 assertion-RED) ---

    #[test]
    fn push_submitted_clears_pasted_and_resets_next_paste_seq() {
        let s0 = paste_sentinel(0);
        let state = InputBufferState {
            text: format!("x{s0}"),
            cursor: format!("x{s0}").len(),
            pasted: BTreeMap::from([(
                s0,
                PastedChunk {
                    seq: 0,
                    text: "folded".to_string(),
                    line_count: 1,
                },
            )]),
            next_paste_seq: 2,
            ..InputBufferState::default()
        };

        let state = reduce(&state, InputBufferAction::PushSubmitted("x".to_string()));

        assert!(state.pasted.is_empty());
        assert_eq!(state.next_paste_seq, 0);
    }

    #[test]
    fn backspace_deleting_sentinel_prunes_pasted_map() {
        let s0 = paste_sentinel(0);
        let text = format!("a{s0}");
        let state = InputBufferState {
            text: text.clone(),
            cursor: "a".len() + s0.len_utf8(),
            pasted: BTreeMap::from([(
                s0,
                PastedChunk {
                    seq: 0,
                    text: "X\nY".to_string(),
                    line_count: 2,
                },
            )]),
            next_paste_seq: 1,
            ..InputBufferState::default()
        };

        let state = reduce(&state, InputBufferAction::Backspace);

        assert!(state.pasted.is_empty());
        assert_eq!(state.text, "a");
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
