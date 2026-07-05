use crate::tui::width::char_width;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InputVisualLayout {
    pub lines: Vec<String>,
    pub line_starts: Vec<usize>,
    pub cursor: VisualPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualPosition {
    pub row: usize,
    pub col: usize,
}

pub(crate) fn input_content_height_cap(
    screen_height: u16,
    status_top_gap_height: u16,
    permission_height: u16,
    max_content_rows: u16,
    queue_height: u16,
    plan_progress_height: u16,
) -> u16 {
    let available = i32::from(screen_height)
        - 16
        - i32::from(status_top_gap_height)
        - i32::from(permission_height)
        - i32::from(queue_height)
        - i32::from(plan_progress_height);
    available.clamp(1, i32::from(max_content_rows)) as u16
}

pub(crate) fn visual_input_layout(
    text: &str,
    cursor: usize,
    inner_width: usize,
) -> InputVisualLayout {
    let cursor = previous_char_boundary(text, cursor.min(text.len()));
    let inner_width = inner_width.max(1);
    let mut lines = Vec::new();
    let mut line_starts = Vec::new();
    let mut cursor_position = None;
    let logical_lines = text.split('\n').collect::<Vec<_>>();
    let mut line_start = 0;

    for logical_line in logical_lines {
        let line_end = line_start + logical_line.len();
        push_visual_line(
            logical_line,
            line_start,
            inner_width,
            cursor,
            &mut lines,
            &mut line_starts,
            &mut cursor_position,
        );
        line_start = line_end.saturating_add(1);
    }

    InputVisualLayout {
        lines,
        line_starts,
        cursor: cursor_position.unwrap_or(VisualPosition { row: 0, col: 0 }),
    }
}

pub(crate) fn input_scroll_offset(
    visual_line_count: usize,
    cap: usize,
    cursor_visual_row: usize,
) -> usize {
    let cap = cap.max(1);
    cursor_visual_row
        .saturating_sub(cap.saturating_sub(1))
        .min(visual_line_count.saturating_sub(cap))
}

fn push_visual_line(
    logical_line: &str,
    line_start: usize,
    inner_width: usize,
    cursor: usize,
    lines: &mut Vec<String>,
    line_starts: &mut Vec<usize>,
    cursor_position: &mut Option<VisualPosition>,
) {
    let mut current = String::new();
    let mut current_col = 0;
    let mut current_start = line_start;
    let capacity = inner_width.max(1);
    let mut row = lines.len();

    if logical_line.is_empty() {
        lines.push(String::new());
        line_starts.push(line_start);
        if cursor == line_start && cursor_position.is_none() {
            *cursor_position = Some(VisualPosition { row, col: 0 });
        }
        return;
    }

    for (offset, ch) in logical_line.char_indices() {
        let index = line_start + offset;
        let width = char_width(ch);
        if !current.is_empty() && current_col + width > capacity {
            lines.push(std::mem::take(&mut current));
            line_starts.push(current_start);
            current_col = 0;
            row = lines.len();
            current_start = index;
        }

        if cursor == index && cursor_position.is_none() {
            *cursor_position = Some(VisualPosition {
                row,
                col: current_col,
            });
        }

        current.push(ch);
        current_col += width;

        let next_index = index + ch.len_utf8();
        if cursor == next_index && current_col < capacity && cursor_position.is_none() {
            *cursor_position = Some(VisualPosition {
                row,
                col: current_col,
            });
        }
    }

    lines.push(current);
    line_starts.push(current_start);
    let at_line_end = cursor == line_start + logical_line.len() && cursor_position.is_none();
    if current_col == capacity {
        if at_line_end {
            lines.push(String::new());
            line_starts.push(line_start + logical_line.len());
            *cursor_position = Some(VisualPosition {
                row: lines.len() - 1,
                col: 0,
            });
        }
    } else if at_line_end {
        *cursor_position = Some(VisualPosition {
            row: lines.len() - 1,
            col: current_col,
        });
    }
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    if text.is_char_boundary(cursor) {
        return cursor;
    }
    let mut previous = 0;
    for (index, _) in text.char_indices() {
        if index > cursor {
            break;
        }
        previous = index;
    }
    previous
}

#[cfg(test)]
mod tests {
    use super::{
        input_content_height_cap, input_scroll_offset, visual_input_layout, VisualPosition,
    };

    #[test]
    fn input_height_cap_reserves_fixed_rows_transcript_floor_and_limits() {
        assert_eq!(input_content_height_cap(26, 2, 0, 10, 0, 0), 8);
        assert_eq!(input_content_height_cap(40, 2, 0, 10, 0, 0), 10);
        assert_eq!(input_content_height_cap(6, 0, 12, 10, 0, 0), 1);
    }

    #[test]
    fn input_height_cap_subtracts_queue_height() {
        assert_eq!(input_content_height_cap(26, 2, 0, 10, 0, 0), 8);
        assert_eq!(input_content_height_cap(26, 2, 0, 10, 3, 0), 5);
    }

    #[test]
    fn input_height_cap_min_screen_with_max_queue_preserves_transcript_floor() {
        const QUEUE_MAX_ROWS: u16 = 5;
        let cap = input_content_height_cap(24, 2, 0, 10, QUEUE_MAX_ROWS, 0);
        assert_eq!(cap, 1);
        // header3 + transcript_min8 + gap2 + activity1 + queue5 + input3 + status1 + mode1 = 24
        let fixed_rows = 3 + 8 + 2 + 1 + QUEUE_MAX_ROWS + 3 + 1 + 1;
        assert_eq!(fixed_rows, 24);
    }

    #[test]
    fn input_height_cap_shrinks_when_permission_box_is_visible() {
        let normal = input_content_height_cap(30, 2, 0, 10, 0, 0);
        let pending_permission = input_content_height_cap(30, 0, 9, 10, 0, 0);

        assert_eq!(normal, 10);
        assert_eq!(pending_permission, 5);
        assert!(pending_permission < normal);
    }

    #[test]
    fn visual_layout_maps_logical_lines_soft_wraps_and_places_cursor() {
        let multiline = visual_input_layout("ab\ncd", "ab\nc".len(), 6);
        assert_eq!(multiline.lines, vec!["ab".to_string(), "cd".to_string()]);
        assert_eq!(multiline.cursor, VisualPosition { row: 1, col: 1 });

        let wrapped = visual_input_layout("abcdefghi", "abcdefghi".len(), 4);
        assert_eq!(
            wrapped.lines,
            vec!["abcd".to_string(), "efgh".to_string(), "i".to_string()]
        );
        assert_eq!(wrapped.cursor, VisualPosition { row: 2, col: 1 });

        let cjk = visual_input_layout("你a好", "你a好".len(), 3);
        assert_eq!(cjk.lines, vec!["你a".to_string(), "好".to_string()]);
        assert_eq!(cjk.cursor, VisualPosition { row: 1, col: 2 });

        let boundary = visual_input_layout("abcd", "abcd".len(), 4);
        assert_eq!(boundary.lines, vec!["abcd".to_string(), "".to_string()]);
        assert_eq!(boundary.cursor, VisualPosition { row: 1, col: 0 });
    }

    #[test]
    fn visual_layout_only_adds_boundary_row_for_cursor_at_full_width_boundary() {
        let after = visual_input_layout("abcd\nef", "abcd\nef".len(), 4);
        assert_eq!(after.lines, vec!["abcd".to_string(), "ef".to_string()]);
        assert_eq!(after.cursor, VisualPosition { row: 1, col: 2 });

        let at = visual_input_layout("abcd\nef", "abcd".len(), 4);
        assert_eq!(
            at.lines,
            vec!["abcd".to_string(), "".to_string(), "ef".to_string()]
        );
        assert_eq!(at.cursor, VisualPosition { row: 1, col: 0 });
    }

    #[test]
    fn visual_layout_line_starts_match_each_visual_line() {
        let cases = [
            ("multiple logical lines", "ab\ncd", "ab\nc".len(), 6),
            ("soft wrap", "abcdefghi", "abcdefghi".len(), 4),
            ("cjk wrap", "你a好", "你a好".len(), 3),
            ("empty logical line", "ab\n\ncd", "ab\n".len(), 6),
            ("full boundary cursor row", "abcd", "abcd".len(), 4),
        ];

        for (name, text, cursor, width) in cases {
            let layout = visual_input_layout(text, cursor, width);
            assert_eq!(
                layout.lines.len(),
                layout.line_starts.len(),
                "{name}: every visual line needs a start offset"
            );
            for (line, start) in layout.lines.iter().zip(layout.line_starts.iter()) {
                assert_eq!(
                    &text[*start..start + line.len()],
                    line,
                    "{name}: line {line:?} must be a contiguous display-text slice"
                );
            }
        }
    }

    #[test]
    fn input_scroll_offset_keeps_cursor_visual_row_visible() {
        assert_eq!(input_scroll_offset(8, 3, 0), 0);
        assert_eq!(input_scroll_offset(8, 3, 4), 2);
        assert_eq!(input_scroll_offset(8, 3, 7), 5);

        for (cursor_row, offset) in [(0, 0), (4, 2), (7, 5)] {
            assert!(cursor_row >= offset);
            assert!(cursor_row < offset + 3);
        }
    }
}
