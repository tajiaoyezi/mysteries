use crate::tui::app::{AppState, TranscriptBlock};
use crate::tui::render::selection_text;
use ratatui::buffer::Buffer;

pub trait Clipboard {
    fn set_text(&mut self, text: String) -> Result<(), String>;
}

pub struct ArboardClipboard {
    inner: Option<arboard::Clipboard>,
}

impl ArboardClipboard {
    pub fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok(),
        }
    }
}

impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for ArboardClipboard {
    fn set_text(&mut self, text: String) -> Result<(), String> {
        let Some(clipboard) = self.inner.as_mut() else {
            return Err("剪贴板不可用".to_string());
        };
        clipboard.set_text(text).map_err(|err| err.to_string())
    }
}

pub fn copy_selection(
    state: &mut AppState,
    last_frame: Option<&Buffer>,
    clipboard: &mut dyn Clipboard,
) {
    let Some(selection) = state.selection.selection else {
        return;
    };
    let Some(buffer) = last_frame else {
        return;
    };

    let text = selection_text(buffer, &selection);
    if text.trim().is_empty() {
        return;
    }

    if let Err(err) = clipboard.set_text(text) {
        state
            .transcript
            .push(TranscriptBlock::Notice(format!("复制失败: {err}")));
    }
}

#[cfg(test)]
mod tests {
    use super::{copy_selection, Clipboard};
    use crate::tui::app::{AppState, TranscriptBlock};
    use crate::tui::selection::{Point, SelectionAction};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    struct MockClipboard {
        calls: Vec<String>,
        result: Result<(), String>,
    }

    impl Default for MockClipboard {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                result: Ok(()),
            }
        }
    }

    impl Clipboard for MockClipboard {
        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.calls.push(text);
            self.result.clone()
        }
    }

    fn point(col: u16, row: u16) -> Point {
        Point { col, row }
    }

    fn select(state: &mut AppState, start_col: u16, end_col: u16) {
        state.apply_selection_action(SelectionAction::Press(point(start_col, 0)));
        state.apply_selection_action(SelectionAction::Drag(point(end_col, 0)));
        state.apply_selection_action(SelectionAction::Release(point(end_col, 0)));
    }

    fn buffer_with_text(text: &str) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
        buffer.set_string(0, 0, text, Style::default());
        buffer
    }

    #[test]
    fn copy_selection_sets_clipboard_text_and_keeps_selection() {
        let mut state = AppState::new();
        select(&mut state, 0, 4);
        let buffer = buffer_with_text("hello   ");
        let mut clipboard = MockClipboard::default();

        copy_selection(&mut state, Some(&buffer), &mut clipboard);

        assert_eq!(clipboard.calls, vec!["hello".to_string()]);
        assert!(state.has_selection());
    }

    #[test]
    fn copy_selection_failure_adds_notice_and_keeps_selection() {
        let mut state = AppState::new();
        select(&mut state, 0, 4);
        let buffer = buffer_with_text("hello");
        let mut clipboard = MockClipboard {
            calls: Vec::new(),
            result: Err("clipboard unavailable".to_string()),
        };

        copy_selection(&mut state, Some(&buffer), &mut clipboard);

        assert!(state.has_selection());
        assert!(
            state.transcript.iter().any(|block| {
                matches!(
                    block,
                    TranscriptBlock::Notice(text)
                        if text.contains("复制失败") && text.contains("clipboard unavailable")
                )
            }),
            "expected copy failure notice in transcript"
        );
        assert_eq!(clipboard.calls, vec!["hello".to_string()]);
    }

    #[test]
    fn copy_selection_skips_empty_or_blank_text_without_touching_clipboard() {
        let mut empty_state = AppState::new();
        select(&mut empty_state, 0, 4);
        let empty_buffer = buffer_with_text("");
        let mut empty_clipboard = MockClipboard::default();

        copy_selection(&mut empty_state, Some(&empty_buffer), &mut empty_clipboard);

        assert!(empty_clipboard.calls.is_empty());
        assert!(empty_state.has_selection());

        let mut blank_state = AppState::new();
        select(&mut blank_state, 0, 4);
        let blank_buffer = buffer_with_text("     ");
        let mut blank_clipboard = MockClipboard::default();

        copy_selection(&mut blank_state, Some(&blank_buffer), &mut blank_clipboard);

        assert!(blank_clipboard.calls.is_empty());
        assert!(blank_state.has_selection());
    }

    #[test]
    fn copy_selection_skips_when_last_frame_is_missing_without_touching_clipboard() {
        let mut state = AppState::new();
        select(&mut state, 0, 4);
        let mut clipboard = MockClipboard::default();

        copy_selection(&mut state, None, &mut clipboard);

        assert!(clipboard.calls.is_empty());
        assert!(state.has_selection());
    }
}