use crate::tui::app::{AppState, TranscriptBlock};
use crate::tui::render::selection_text;
use ratatui::buffer::Buffer;

pub trait Clipboard {
    fn set_text(&mut self, text: String) -> Result<(), String>;
    fn get_text(&mut self) -> Result<String, String>;
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

    fn get_text(&mut self) -> Result<String, String> {
        let Some(clipboard) = self.inner.as_mut() else {
            return Err("剪贴板不可用".to_string());
        };
        clipboard.get_text().map_err(|err| err.to_string())
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
    let char_count = text.chars().count();
    match clipboard.set_text(text) {
        // 成功走 activity line 右侧轻提示(不入 transcript,防高频复制刷屏);
        // 失败留 transcript Notice(异常留痕,spec 锁定)。
        Ok(()) => state.set_copy_hint(format!("已复制 {char_count} 字")),
        Err(err) => state
            .transcript
            .push(TranscriptBlock::Notice(format!("复制失败: {err}"))),
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
        get_result: Result<String, String>,
    }

    impl Default for MockClipboard {
        fn default() -> Self {
            Self {
                calls: Vec::new(),
                result: Ok(()),
                get_result: Ok(String::new()),
            }
        }
    }

    impl Clipboard for MockClipboard {
        fn set_text(&mut self, text: String) -> Result<(), String> {
            self.calls.push(text);
            self.result.clone()
        }

        fn get_text(&mut self) -> Result<String, String> {
            self.get_result.clone()
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
        let blocks_before = state.transcript.len();

        copy_selection(&mut state, Some(&buffer), &mut clipboard);

        assert_eq!(clipboard.calls, vec!["hello".to_string()]);
        assert!(state.has_selection());
        assert_eq!(
            state.active_copy_hint(std::time::Instant::now()),
            Some("已复制 5 字"),
            "success must set the activity-line copy hint"
        );
        assert_eq!(
            state.transcript.len(),
            blocks_before,
            "success copy must not append transcript notices"
        );
    }

    #[test]
    fn copy_selection_success_hint_counts_chars_not_bytes_for_cjk() {
        let mut state = AppState::new();
        select(&mut state, 0, 3);
        let buffer = buffer_with_text("你好");
        let mut clipboard = MockClipboard::default();
        let blocks_before = state.transcript.len();

        copy_selection(&mut state, Some(&buffer), &mut clipboard);

        assert_eq!(clipboard.calls, vec!["你好".to_string()]);
        assert_eq!(
            state.active_copy_hint(std::time::Instant::now()),
            Some("已复制 2 字"),
            "hint must count chars, not bytes"
        );
        assert_eq!(
            state.transcript.len(),
            blocks_before,
            "success copy must not append transcript notices"
        );
    }

    #[test]
    fn copy_selection_failure_adds_notice_and_keeps_selection() {
        let mut state = AppState::new();
        select(&mut state, 0, 4);
        let buffer = buffer_with_text("hello");
        let mut clipboard = MockClipboard {
            calls: Vec::new(),
            result: Err("clipboard unavailable".to_string()),
            get_result: Ok(String::new()),
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

    #[test]
    fn mock_clipboard_get_text_returns_configured_text() {
        let mut clipboard = MockClipboard {
            get_result: Ok("from clipboard".to_string()),
            ..MockClipboard::default()
        };

        assert_eq!(clipboard.get_text(), Ok("from clipboard".to_string()));
    }

    #[test]
    fn mock_clipboard_get_text_returns_configured_error() {
        let mut clipboard = MockClipboard {
            get_result: Err("clipboard read failed".to_string()),
            ..MockClipboard::default()
        };

        assert_eq!(
            clipboard.get_text(),
            Err("clipboard read failed".to_string())
        );
    }

    #[test]
    fn arboard_clipboard_get_text_reports_unavailable_when_inner_missing() {
        let mut clipboard = super::ArboardClipboard { inner: None };

        assert_eq!(clipboard.get_text(), Err("剪贴板不可用".to_string()));
    }
}
