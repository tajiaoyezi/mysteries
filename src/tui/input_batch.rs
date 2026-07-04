//! 批次分类纯逻辑:滤除非 Press 键事件 + 按"文本内容键数 n"判定裸 Enter 换行/提交。
//! 见 openspec/changes/guard-paste-burst-submit design.md D2。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub const PASTE_FOLD_MIN_LINES: usize = 15;
pub const PASTE_FOLD_MIN_CHARS: usize = 500;
pub const PASTE_FAST_MIN_MATCH_CHARS: usize = 8;
pub const PASTE_TAIL_ABORT_MISMATCHES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyIntent {
    Newline,
    Submit,
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailAction {
    Drop,
    Forward,
}

#[derive(Debug, Clone)]
pub struct PasteTailMatcher {
    normalized: String,
    cursor: usize,
    in_newline_run: bool,
    forwarded_streak: usize,
    done: bool,
    aborted: bool,
}

impl PasteTailMatcher {
    pub fn new(normalized: String) -> Self {
        let done = normalized.is_empty();
        Self {
            normalized,
            cursor: 0,
            in_newline_run: false,
            forwarded_streak: 0,
            done,
            aborted: false,
        }
    }

    pub fn on_event(&mut self, event: &Event) -> TailAction {
        if self.done {
            return TailAction::Forward;
        }

        let Some(key) = event_key(event) else {
            return TailAction::Forward;
        };
        if !super::is_key_press(key) {
            return TailAction::Drop;
        }

        let Some(actual) = rebuildable_key_char(&key) else {
            return TailAction::Forward;
        };
        if self.aborted {
            return TailAction::Drop;
        }

        if self.in_newline_run {
            if actual == '\n' {
                return TailAction::Drop;
            }
            self.in_newline_run = false;
            if self.cursor >= self.normalized.len() {
                self.done = true;
                return TailAction::Forward;
            }
        }

        let mut skipped_unreliable = false;
        loop {
            let Some(expected) = self.expected_char() else {
                self.done = true;
                return TailAction::Forward;
            };

            if expected == '\n' && actual == '\n' {
                self.cursor = newline_run_end(&self.normalized, self.cursor);
                self.in_newline_run = true;
                self.forwarded_streak = 0;
                return TailAction::Drop;
            }

            if expected == actual {
                self.cursor += expected.len_utf8();
                self.forwarded_streak = 0;
                self.done = self.cursor >= self.normalized.len();
                return TailAction::Drop;
            }

            if !skipped_unreliable && is_unreliable_stream_char(expected) {
                self.skip_unreliable_chars();
                skipped_unreliable = true;
                continue;
            }

            break;
        }

        self.forwarded_streak += 1;
        if self.forwarded_streak >= PASTE_TAIL_ABORT_MISMATCHES {
            self.aborted = true;
        }
        TailAction::Forward
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn normalized_len(&self) -> usize {
        self.normalized.len()
    }

    fn expected_char(&self) -> Option<char> {
        self.normalized[self.cursor..].chars().next()
    }

    fn skip_unreliable_chars(&mut self) {
        while let Some(ch) = self.expected_char() {
            if !is_unreliable_stream_char(ch) {
                break;
            }
            self.cursor += ch.len_utf8();
        }
    }
}

#[derive(Debug, Clone)]
pub struct FastPaste {
    pub fold_text: String,
    pub tail: PasteTailMatcher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastPasteDeclineReason {
    TooShort,
    NoMatch,
    ClipboardErr,
    BelowThreshold,
}

impl FastPasteDeclineReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooShort => "too-short",
            Self::NoMatch => "no-match",
            Self::ClipboardErr => "clipboard-err",
            Self::BelowThreshold => "below-threshold",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FastPasteDecline {
    pub reason: FastPasteDeclineReason,
    pub rebuilt_chars: usize,
    pub batch_len: usize,
}

#[derive(Debug, Clone)]
pub enum FastPasteDecision {
    Matched(FastPaste),
    Declined(FastPasteDecline),
}

/// 从 raw batch 抽取 Press 键事件(Windows Press+Release 双发,Release 不计入)。
pub fn press_key_events(batch: &[Event]) -> Vec<KeyEvent> {
    batch
        .iter()
        .filter_map(|event| match event {
            Event::Key(key) if super::is_key_press(*key) => Some(*key),
            _ => None,
        })
        .collect()
}

/// 裸 Enter:`Enter && !CONTROL && !SHIFT`(modifier 版换行键不归分类器接管)。
fn is_bare_enter(key: &KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::SHIFT)
}

/// 文本内容键(计入 n):Char(排除纯 Ctrl+char;AltGr=CONTROL|ALT 保留)+ 裸 Enter。
fn is_text_content_key(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(_) => {
            !key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT)
        }
        KeyCode::Enter => is_bare_enter(key),
        _ => false,
    }
}

/// 对 Press 键批分类:n = 文本内容键(Char 非纯 Ctrl+char + 裸 Enter)计数;
/// n>=2 裸 Enter→Newline,n==1 裸 Enter→Submit,其余→Passthrough。
pub fn classify_key_batch(keys: &[KeyEvent]) -> Vec<KeyIntent> {
    let n = keys.iter().filter(|key| is_text_content_key(key)).count();
    keys.iter()
        .map(|key| {
            if is_bare_enter(key) {
                if n >= 2 {
                    KeyIntent::Newline
                } else {
                    KeyIntent::Submit
                }
            } else {
                KeyIntent::Passthrough
            }
        })
        .collect()
}

pub fn would_submit_lone_enter(batch: &[Event]) -> bool {
    classify_key_batch(&press_key_events(batch)).contains(&KeyIntent::Submit)
}

pub fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if matches!(chars.peek(), Some('\n')) {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn rebuild_text(keys: &[KeyEvent]) -> Option<String> {
    rebuild_with(keys, text_content_key_char)
}

pub fn rebuild_fast_text(keys: &[KeyEvent]) -> Option<String> {
    rebuild_with(keys, rebuildable_key_char)
}

pub fn fold_candidate(batch: &[Event], min_lines: usize, min_chars: usize) -> Option<String> {
    let keys = press_key_events(batch);
    let text = rebuild_text(&keys)?;
    (text.split('\n').count() >= min_lines || text.chars().count() >= min_chars).then_some(text)
}

pub fn try_fast_paste(
    batch: &[Event],
    read_clipboard: impl FnOnce() -> Result<String, String>,
) -> Option<FastPaste> {
    match try_fast_paste_decision(batch, read_clipboard) {
        FastPasteDecision::Matched(fast) => Some(fast),
        FastPasteDecision::Declined(_) => None,
    }
}

pub fn try_fast_paste_decision(
    batch: &[Event],
    read_clipboard: impl FnOnce() -> Result<String, String>,
) -> FastPasteDecision {
    let keys = press_key_events(batch);
    let batch_len = batch.len();
    let Some(rebuilt) = rebuild_fast_text(&keys) else {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::NoMatch,
            rebuilt_chars: 0,
            batch_len,
        });
    };
    let rebuilt_chars = rebuilt.chars().count();
    if rebuilt_chars < PASTE_FAST_MIN_MATCH_CHARS {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::TooShort,
            rebuilt_chars,
            batch_len,
        });
    }

    let Ok(clipboard) = read_clipboard() else {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::ClipboardErr,
            rebuilt_chars,
            batch_len,
        });
    };
    if clipboard.trim().is_empty() {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::ClipboardErr,
            rebuilt_chars,
            batch_len,
        });
    }
    let normalized = normalize_newlines(&clipboard);
    let mut tail = PasteTailMatcher::new(normalized.clone());
    if batch
        .iter()
        .any(|event| matches!(event, Event::Key(_)) && tail.on_event(event) != TailAction::Drop)
    {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::NoMatch,
            rebuilt_chars,
            batch_len,
        });
    }
    if normalized.split('\n').count() < PASTE_FOLD_MIN_LINES
        && normalized.chars().count() < PASTE_FOLD_MIN_CHARS
    {
        return FastPasteDecision::Declined(FastPasteDecline {
            reason: FastPasteDeclineReason::BelowThreshold,
            rebuilt_chars,
            batch_len,
        });
    }

    FastPasteDecision::Matched(FastPaste {
        fold_text: normalized,
        tail,
    })
}

fn event_key(event: &Event) -> Option<KeyEvent> {
    match event {
        Event::Key(key) => Some(*key),
        _ => None,
    }
}

fn text_content_key_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(ch)
        }
        KeyCode::Enter if is_bare_enter(key) => Some('\n'),
        _ => None,
    }
}

fn rebuildable_key_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(ch)
        }
        KeyCode::Enter if is_bare_enter(key) => Some('\n'),
        KeyCode::Tab if key.modifiers.is_empty() => Some('\t'),
        _ => None,
    }
}

fn rebuild_with(keys: &[KeyEvent], key_char: fn(&KeyEvent) -> Option<char>) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    keys.iter().map(key_char).collect()
}

fn newline_run_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find_map(|(offset, ch)| (ch != '\n').then_some(start + offset))
        .unwrap_or(text.len())
}

fn is_unreliable_stream_char(ch: char) -> bool {
    (ch as u32) > 0xFFFF || ch == '\u{FE0F}' || ch == '\u{200D}'
}

#[cfg(test)]
mod tests {
    use super::{
        classify_key_batch, fold_candidate, normalize_newlines, press_key_events,
        rebuild_fast_text, rebuild_text, try_fast_paste, try_fast_paste_decision,
        would_submit_lone_enter, FastPasteDecision, FastPasteDecline, FastPasteDeclineReason,
        KeyIntent, PasteTailMatcher, TailAction, PASTE_FAST_MIN_MATCH_CHARS,
        PASTE_FOLD_MIN_CHARS, PASTE_FOLD_MIN_LINES, PASTE_TAIL_ABORT_MISMATCHES,
    };
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    };
    use std::cell::Cell;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn press_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ))
    }

    fn release_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ))
    }

    fn press_event_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            modifiers,
            KeyEventKind::Press,
        ))
    }

    fn moved_event() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn paste_events_from_text(text: &str, double_enter: bool) -> Vec<Event> {
        let mut events = Vec::new();
        for ch in text.chars() {
            match ch {
                '\n' => {
                    events.push(press_event(KeyCode::Enter));
                    if double_enter {
                        events.push(press_event(KeyCode::Enter));
                    }
                }
                '\t' => events.push(press_event(KeyCode::Tab)),
                ch => events.push(press_event(KeyCode::Char(ch))),
            }
        }
        events
    }

    fn tail_actions(normalized: &str, events: Vec<Event>) -> (Vec<TailAction>, PasteTailMatcher) {
        let mut matcher = PasteTailMatcher::new(normalized.to_string());
        let actions = events.iter().map(|event| matcher.on_event(event)).collect();
        (actions, matcher)
    }

    /// 构造 N 逻辑行粘贴批:每行一个 Char,行间裸 Enter(N−1 个),无尾随 Enter。
    fn paste_lines_batch(line_count: usize, ch: char) -> Vec<Event> {
        let mut batch = Vec::with_capacity(line_count * 2 - 1);
        for i in 0..line_count {
            if i > 0 {
                batch.push(press_event(KeyCode::Enter));
            }
            batch.push(press_event(KeyCode::Char(ch)));
        }
        batch
    }

    fn paste_chars_batch(count: usize, ch: char) -> Vec<Event> {
        std::iter::repeat_with(|| press_event(KeyCode::Char(ch)))
            .take(count)
            .collect()
    }

    fn paste_fixed_width_lines_batch(line_count: usize, chars_per_line: usize) -> Vec<Event> {
        let mut batch = Vec::new();
        for line in 0..line_count {
            if line > 0 {
                batch.push(press_event(KeyCode::Enter));
            }
            batch.extend((0..chars_per_line).map(|_| press_event(KeyCode::Char('x'))));
        }
        batch
    }

    // --- Task 2.1 RED: fold_candidate ---

    #[test]
    fn fold_candidate_returns_some_when_line_count_meets_threshold() {
        let batch = paste_lines_batch(15, 'x');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);
        let s = result.expect("15 logical lines should fold");
        assert_eq!(s.split('\n').count(), 15);
    }

    #[test]
    fn fold_candidate_returns_none_when_line_count_below_threshold() {
        let batch = paste_lines_batch(14, 'x');
        assert_eq!(
            fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS),
            None
        );
    }

    #[test]
    fn fold_candidate_returns_none_when_batch_contains_non_text_key() {
        let mut batch = paste_lines_batch(15, 'x');
        batch.insert(3, press_event(KeyCode::PageUp));
        assert_eq!(
            fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS),
            None
        );
    }

    #[test]
    fn fold_candidate_returns_none_for_empty_batch() {
        assert_eq!(
            fold_candidate(&[], PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS),
            None
        );
    }

    #[test]
    fn fold_candidate_rebuilds_cjk_lines_with_bare_enter_as_newline() {
        let batch = paste_lines_batch(15, '你');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);
        let expected = std::iter::repeat_n("你", 15).collect::<Vec<_>>().join("\n");
        assert_eq!(result.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn fold_candidate_returns_some_for_single_line_above_char_threshold() {
        let batch = paste_chars_batch(600, 'x');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);

        assert!(
            result.is_some(),
            "single-line paste with 600 chars should fold"
        );
    }

    #[test]
    fn fold_candidate_returns_some_for_single_line_at_char_threshold() {
        let batch = paste_chars_batch(PASTE_FOLD_MIN_CHARS, 'x');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);

        assert!(
            result.is_some(),
            "single-line paste at 500 chars should fold"
        );
    }

    #[test]
    fn fold_candidate_returns_none_for_single_line_below_char_threshold() {
        let batch = paste_chars_batch(PASTE_FOLD_MIN_CHARS - 1, 'x');

        assert_eq!(
            fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS),
            None
        );
    }

    #[test]
    fn fold_candidate_returns_some_for_multiline_below_line_threshold_above_char_threshold() {
        let batch = paste_fixed_width_lines_batch(14, 40);
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);

        assert!(
            result.is_some(),
            "14 lines x 40 chars should fold by character threshold"
        );
    }

    #[test]
    fn normalize_newlines_converts_crlf_cr_mixed_and_trailing_breaks() {
        assert_eq!(normalize_newlines("a\r\nb"), "a\nb");
        assert_eq!(normalize_newlines("a\rb"), "a\nb");
        assert_eq!(normalize_newlines("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(normalize_newlines("a\r\n"), "a\n");
    }

    #[test]
    fn rebuild_fast_text_accepts_tab_as_rebuildable_key() {
        let keys = [
            key(KeyCode::Char('a')),
            key(KeyCode::Tab),
            key(KeyCode::Char('b')),
        ];

        assert_eq!(rebuild_fast_text(&keys).as_deref(), Some("a\tb"));
    }

    #[test]
    fn rebuild_text_rejects_page_up_boundary() {
        let keys = [key(KeyCode::Char('a')), key(KeyCode::PageUp)];

        assert_eq!(rebuild_text(&keys), None);
    }

    #[test]
    fn fold_candidate_rejects_tab_even_when_thresholds_are_met() {
        let mut batch = paste_chars_batch(PASTE_FOLD_MIN_CHARS, 'x');
        batch.push(press_event(KeyCode::Tab));

        assert_eq!(
            fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS),
            None
        );
    }

    #[test]
    fn matcher_drops_matching_chars_and_reports_done() {
        let (actions, matcher) = tail_actions("abc", paste_events_from_text("abc", false));

        assert_eq!(
            actions,
            vec![TailAction::Drop, TailAction::Drop, TailAction::Drop]
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_drops_newline_runs_for_single_and_double_enter_shapes() {
        let (single_actions, single) = tail_actions("a\nb", paste_events_from_text("a\nb", false));
        let (double_actions, double) = tail_actions("a\nb", paste_events_from_text("a\nb", true));

        assert_eq!(
            single_actions,
            vec![TailAction::Drop, TailAction::Drop, TailAction::Drop]
        );
        assert!(single.is_done());
        assert_eq!(
            double_actions,
            vec![
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop
            ]
        );
        assert!(double.is_done());
    }

    #[test]
    fn matcher_drops_empty_line_runs_and_tab_matches() {
        let (empty_line_actions, empty_line) =
            tail_actions("a\n\nb", paste_events_from_text("a\n\nb", true));
        let (tab_actions, tab) = tail_actions("a\tb", paste_events_from_text("a\tb", false));

        assert_eq!(
            empty_line_actions,
            vec![
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop
            ]
        );
        assert!(empty_line.is_done());
        assert_eq!(
            tab_actions,
            vec![TailAction::Drop, TailAction::Drop, TailAction::Drop]
        );
        assert!(tab.is_done());
    }

    #[test]
    fn matcher_drops_empty_line_run_single_enter_shape() {
        let (actions, matcher) = tail_actions("a\n\nb", paste_events_from_text("a\n\nb", false));

        assert_eq!(
            actions,
            vec![
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop
            ]
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_mismatch_forwards_without_advancing_and_resyncs() {
        let (actions, matcher) = tail_actions(
            "abc",
            vec![
                press_event(KeyCode::Char('x')),
                press_event(KeyCode::Char('a')),
                press_event(KeyCode::Char('b')),
                press_event(KeyCode::Char('c')),
            ],
        );

        assert_eq!(
            actions,
            vec![
                TailAction::Forward,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop
            ]
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_controls_and_non_key_events_forward_without_touching_state() {
        let (actions, matcher) = tail_actions(
            "c\nb",
            vec![
                press_event_with_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL),
                press_event(KeyCode::Char('c')),
                press_event(KeyCode::Enter),
                Event::Resize(80, 24),
                press_event(KeyCode::Enter),
                press_event(KeyCode::Char('b')),
                press_event(KeyCode::Esc),
                press_event(KeyCode::Left),
            ],
        );

        assert_eq!(
            actions,
            vec![
                TailAction::Forward,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Forward,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Forward,
                TailAction::Forward
            ]
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_release_is_always_dropped_without_state_change() {
        let (actions, matcher) = tail_actions(
            "a",
            vec![
                release_event(KeyCode::Char('a')),
                press_event(KeyCode::Char('a')),
            ],
        );

        assert_eq!(actions, vec![TailAction::Drop, TailAction::Drop]);
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_done_forwards_remaining_events_after_content_end() {
        let mut matcher = PasteTailMatcher::new("ab".to_string());

        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('b'))),
            TailAction::Drop
        );
        assert!(matcher.is_done());
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('c'))),
            TailAction::Forward
        );
    }

    #[test]
    fn matcher_tail_newline_run_absorbs_residual_enter_before_done() {
        let mut matcher = PasteTailMatcher::new("a\n".to_string());

        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Enter)),
            TailAction::Drop
        );
        assert!(
            !matcher.is_done(),
            "tail newline run must absorb before done"
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Enter)),
            TailAction::Drop
        );
        assert!(!matcher.is_done());
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('x'))),
            TailAction::Forward
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn matcher_aborts_after_sixteen_consecutive_mismatches() {
        let mut matcher = PasteTailMatcher::new("a".to_string());

        for _ in 0..PASTE_TAIL_ABORT_MISMATCHES {
            assert_eq!(
                matcher.on_event(&press_event(KeyCode::Char('x'))),
                TailAction::Forward
            );
        }
        assert!(
            !matcher.is_done(),
            "abort must enter guard mode and wait for the 2s quiet fallback"
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('x'))),
            TailAction::Drop,
            "guard mode must keep rebuildable paste flood out of the input box"
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop,
            "guard mode drops rebuildable keys even when they match the cursor"
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Esc)),
            TailAction::Forward
        );
        assert_eq!(
            matcher.on_event(&moved_event()),
            TailAction::Forward
        );
    }

    #[test]
    fn matcher_matching_event_resets_forwarded_streak() {
        let mut matcher = PasteTailMatcher::new(format!("a{}", "b".repeat(15)));

        for _ in 0..PASTE_TAIL_ABORT_MISMATCHES - 1 {
            assert_eq!(
                matcher.on_event(&press_event(KeyCode::Char('x'))),
                TailAction::Forward
            );
        }
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop
        );
        for _ in 0..PASTE_TAIL_ABORT_MISMATCHES - 1 {
            assert_eq!(
                matcher.on_event(&press_event(KeyCode::Char('x'))),
                TailAction::Forward
            );
        }
        assert!(!matcher.is_done());
    }

    #[test]
    fn unreliable_astral_flag_can_be_swallowed_without_tail_leak() {
        let (actions, matcher) =
            tail_actions("🇭🇰 GOMA", paste_events_from_text(" GOMA", false));

        assert_eq!(actions, vec![TailAction::Drop; 5]);
        assert!(matcher.is_done());
    }

    #[test]
    fn unreliable_astral_before_newline_run_keeps_single_and_double_enter_matching() {
        let (single_actions, single) =
            tail_actions("🇭🇰\nG", paste_events_from_text("\nG", false));
        let (double_actions, double) =
            tail_actions("🇭🇰\nG", paste_events_from_text("\nG", true));

        assert_eq!(single_actions, vec![TailAction::Drop, TailAction::Drop]);
        assert!(single.is_done());
        assert_eq!(
            double_actions,
            vec![TailAction::Drop, TailAction::Drop, TailAction::Drop]
        );
        assert!(double.is_done());
    }

    #[test]
    fn unreliable_astral_at_tail_marks_done_when_next_event_arrives() {
        let mut matcher = PasteTailMatcher::new("a🇭🇰".to_string());

        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('x'))),
            TailAction::Forward
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn unreliable_variation_selector_and_zwj_are_skipped_like_astral_chars() {
        let (vs_actions, vs_matcher) =
            tail_actions("a\u{FE0F}b", paste_events_from_text("ab", false));
        let (zwj_actions, zwj_matcher) =
            tail_actions("a\u{200D}b", paste_events_from_text("ab", false));

        assert_eq!(vs_actions, vec![TailAction::Drop, TailAction::Drop]);
        assert!(vs_matcher.is_done());
        assert_eq!(zwj_actions, vec![TailAction::Drop, TailAction::Drop]);
        assert!(zwj_matcher.is_done());
    }

    #[test]
    fn unreliable_consecutive_emoji_are_skipped_as_one_expected_run() {
        let (actions, matcher) =
            tail_actions("🇭🇰🇺🇸 x", paste_events_from_text(" x", false));

        assert_eq!(actions, vec![TailAction::Drop, TailAction::Drop]);
        assert!(matcher.is_done());
    }

    #[test]
    fn unreliable_real_astral_event_uses_normal_match_path_boundary() {
        let (actions, matcher) =
            tail_actions("🇭🇰 GOMA", paste_events_from_text("🇭🇰 GOMA", false));

        assert_eq!(
            actions,
            vec![
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop,
                TailAction::Drop
            ]
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn unreliable_skip_then_retry_mismatch_forwards_and_can_resync() {
        let mut matcher = PasteTailMatcher::new("🇭🇰a".to_string());

        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('x'))),
            TailAction::Forward
        );
        assert_eq!(
            matcher.on_event(&press_event(KeyCode::Char('a'))),
            TailAction::Drop
        );
        assert!(matcher.is_done());
    }

    #[test]
    fn try_fast_paste_rejects_non_rebuildable_and_short_batch_without_reading_clipboard() {
        let calls = Cell::new(0);
        let mut non_rebuildable = paste_events_from_text("abcdefghi", false);
        non_rebuildable.insert(1, press_event(KeyCode::PageUp));

        assert!(try_fast_paste(&non_rebuildable, || {
            calls.set(calls.get() + 1);
            Ok("abcdefghi".to_string())
        })
        .is_none());
        assert_eq!(calls.get(), 0);

        assert!(
            try_fast_paste(&paste_events_from_text("abcdefg", false), || {
                calls.set(calls.get() + 1);
                Ok("abcdefg".to_string())
            })
            .is_none()
        );
        assert_eq!(
            calls.get(),
            0,
            "7-char batches must reject before reading clipboard; min={PASTE_FAST_MIN_MATCH_CHARS}"
        );
    }

    #[test]
    fn try_fast_paste_rejects_clipboard_error_blank_mismatch_and_below_threshold() {
        assert!(
            try_fast_paste(&paste_events_from_text("abcdefgh", false), || Err(
                "clipboard failed".to_string()
            ))
            .is_none()
        );
        assert!(
            try_fast_paste(&paste_events_from_text("abcdefgh", false), || Ok(
                "   \n\t  ".to_string()
            ))
            .is_none()
        );
        assert!(
            try_fast_paste(&paste_events_from_text("abcdefgh", false), || Ok(
                "zzzzzzzz".to_string()
            ))
            .is_none()
        );

        let below_threshold = paste_fixed_width_lines_batch(14, 34);
        let below_clipboard = (0..14)
            .map(|_| "x".repeat(34))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(try_fast_paste(&below_threshold, || Ok(below_clipboard)).is_none());
    }

    #[test]
    fn try_fast_paste_decision_reports_decline_reasons_without_content() {
        fn decline(decision: FastPasteDecision) -> FastPasteDecline {
            match decision {
                FastPasteDecision::Declined(decline) => decline,
                FastPasteDecision::Matched(_) => panic!("expected decline"),
            }
        }

        assert_eq!(FastPasteDeclineReason::TooShort.as_str(), "too-short");
        assert_eq!(FastPasteDeclineReason::NoMatch.as_str(), "no-match");
        assert_eq!(FastPasteDeclineReason::ClipboardErr.as_str(), "clipboard-err");
        assert_eq!(
            FastPasteDeclineReason::BelowThreshold.as_str(),
            "below-threshold"
        );

        let calls = Cell::new(0);
        let too_short = paste_events_from_text("abcdefg", false);
        let too_short_decline = decline(try_fast_paste_decision(&too_short, || {
            calls.set(calls.get() + 1);
            Ok("abcdefg".to_string())
        }));
        assert_eq!(too_short_decline.reason, FastPasteDeclineReason::TooShort);
        assert_eq!(too_short_decline.rebuilt_chars, 7);
        assert_eq!(too_short_decline.batch_len, too_short.len());
        assert_eq!(calls.get(), 0, "too-short must not read clipboard");

        let mut non_rebuildable = paste_events_from_text("abcdefghi", false);
        non_rebuildable.insert(1, press_event(KeyCode::PageUp));
        let non_rebuildable_decline = decline(try_fast_paste_decision(&non_rebuildable, || {
            calls.set(calls.get() + 1);
            Ok("abcdefghi".to_string())
        }));
        assert_eq!(
            non_rebuildable_decline.reason,
            FastPasteDeclineReason::NoMatch
        );
        assert_eq!(non_rebuildable_decline.rebuilt_chars, 0);
        assert_eq!(non_rebuildable_decline.batch_len, non_rebuildable.len());
        assert_eq!(calls.get(), 0, "non-rebuildable must not read clipboard");

        let clipboard_err_decline = decline(try_fast_paste_decision(
            &paste_events_from_text("abcdefgh", false),
            || Err("clipboard failed".to_string()),
        ));
        assert_eq!(
            clipboard_err_decline.reason,
            FastPasteDeclineReason::ClipboardErr
        );
        assert_eq!(clipboard_err_decline.rebuilt_chars, 8);

        let blank_decline = decline(try_fast_paste_decision(
            &paste_events_from_text("abcdefgh", false),
            || Ok("   \n\t  ".to_string()),
        ));
        assert_eq!(blank_decline.reason, FastPasteDeclineReason::ClipboardErr);
        assert_eq!(blank_decline.rebuilt_chars, 8);

        let no_match_decline = decline(try_fast_paste_decision(
            &paste_events_from_text("abcdefgh", false),
            || Ok("zzzzzzzz".to_string()),
        ));
        assert_eq!(no_match_decline.reason, FastPasteDeclineReason::NoMatch);
        assert_eq!(no_match_decline.rebuilt_chars, 8);

        let below_threshold = paste_fixed_width_lines_batch(14, 34);
        let below_clipboard = (0..14)
            .map(|_| "x".repeat(34))
            .collect::<Vec<_>>()
            .join("\n");
        let below_threshold_decline =
            decline(try_fast_paste_decision(&below_threshold, || Ok(below_clipboard)));
        assert_eq!(
            below_threshold_decline.reason,
            FastPasteDeclineReason::BelowThreshold
        );
        assert_eq!(below_threshold_decline.rebuilt_chars, 489);
        assert_eq!(below_threshold_decline.batch_len, below_threshold.len());
    }

    #[test]
    fn try_fast_paste_returns_normalized_fold_text_for_crlf_single_enter_shape() {
        let clipboard = format!("abcdefgh\r\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let normalized = format!("abcdefgh\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let batch = paste_events_from_text("abcdefgh\nxx", false);

        let fast = try_fast_paste(&batch, || Ok(clipboard)).expect("fast paste should match");

        assert_eq!(fast.fold_text, normalized);
        assert_eq!(fast.fold_text.split('\n').count(), 2);
    }

    #[test]
    fn try_fast_paste_matches_crlf_double_enter_shape() {
        let clipboard = format!("abcdefgh\r\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let batch = paste_events_from_text("abcdefgh\nxx", true);

        assert!(try_fast_paste(&batch, || Ok(clipboard)).is_some());
    }

    #[test]
    fn try_fast_paste_ignores_non_key_events_while_matching_prefix() {
        let clipboard = format!("abcdefgh\r\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let mut batch = paste_events_from_text("abcdefgh\nxx", false);
        batch.insert(2, moved_event());
        batch.insert(5, Event::Resize(80, 24));

        assert!(try_fast_paste(&batch, || Ok(clipboard)).is_some());
    }

    #[test]
    fn unreliable_try_fast_paste_matches_swallowed_emoji_prefix_and_preserves_fold_text() {
        let clipboard = format!("- name: '🇭🇰 GOMA-HK'\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let batch = paste_events_from_text("- name: ' GOMA-HK'\nxx", false);

        let fast =
            try_fast_paste(&batch, || Ok(clipboard.clone())).expect("emoji prefix should match");

        assert_eq!(fast.fold_text, clipboard);
        assert!(fast.fold_text.contains("🇭🇰"));
    }

    #[test]
    fn try_fast_paste_transfers_absorbing_tail_when_batch_ends_on_newline_run() {
        let clipboard = format!("abcdefgh\r\n{}", "x".repeat(PASTE_FOLD_MIN_CHARS));
        let batch = paste_events_from_text("abcdefgh\n", false);

        let mut fast = try_fast_paste(&batch, || Ok(clipboard)).expect("fast paste should match");

        assert_eq!(
            fast.tail.on_event(&press_event(KeyCode::Enter)),
            TailAction::Drop,
            "transferred matcher must still absorb a residual double-Enter newline"
        );
    }

    // ① Windows 孤立 Enter = [Press, Release]:滤 Release 后只剩 1 键,n 不翻倍,判 Submit
    #[test]
    fn press_key_events_drops_release_so_lone_enter_stays_submit() {
        let batch = [press_event(KeyCode::Enter), release_event(KeyCode::Enter)];

        let keys = press_key_events(&batch);
        assert_eq!(keys, vec![key(KeyCode::Enter)]);
        assert_eq!(classify_key_batch(&keys), vec![KeyIntent::Submit]);
    }

    // ② 突发批 [Char a, Enter, Char b](n=3):裸 Enter 归换行,Char 透传
    #[test]
    fn bare_enter_inside_char_burst_classifies_as_newline() {
        let keys = [
            key(KeyCode::Char('a')),
            key(KeyCode::Enter),
            key(KeyCode::Char('b')),
        ];
        assert_eq!(
            classify_key_batch(&keys),
            vec![
                KeyIntent::Passthrough,
                KeyIntent::Newline,
                KeyIntent::Passthrough
            ]
        );
    }

    // ③ 孤立裸 Enter(n=1)→ Submit
    #[test]
    fn lone_bare_enter_classifies_as_submit() {
        assert_eq!(
            classify_key_batch(&[key(KeyCode::Enter)]),
            vec![KeyIntent::Submit]
        );
    }

    // ④ PageUp 非文本内容键、不计入 n → 同批裸 Enter 仍 Submit(n=1)
    #[test]
    fn non_text_key_does_not_count_so_enter_after_pageup_stays_submit() {
        let keys = [key(KeyCode::PageUp), key(KeyCode::Enter)];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Passthrough, KeyIntent::Submit]
        );
    }

    // ⑤ modifier 换行键不受判定接管:Enter+CONTROL → Passthrough,且不计入 n
    #[test]
    fn ctrl_enter_passes_through_and_does_not_count_toward_burst() {
        let ctrl_enter = key_with_modifiers(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(
            classify_key_batch(&[ctrl_enter]),
            vec![KeyIntent::Passthrough]
        );

        let keys = [ctrl_enter, key(KeyCode::Enter)];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Passthrough, KeyIntent::Submit]
        );
    }

    // ⑤b(review 补,tasks 1.1「Enter+CONTROL/SHIFT→Passthrough」的 SHIFT 半边)
    //    Shift+Enter → Passthrough 且不计入 n(只查 !CONTROL 的错误实现过不了)
    #[test]
    fn shift_enter_passes_through_and_does_not_count_toward_burst() {
        let shift_enter = key_with_modifiers(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(
            classify_key_batch(&[shift_enter]),
            vec![KeyIntent::Passthrough]
        );

        let keys = [shift_enter, key(KeyCode::Enter)];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Passthrough, KeyIntent::Submit]
        );
    }

    // ⑥ 空批 → 空向量(两个函数)
    #[test]
    fn empty_batch_yields_empty_outputs() {
        assert_eq!(press_key_events(&[]), Vec::<KeyEvent>::new());
        assert_eq!(classify_key_batch(&[]), Vec::<KeyIntent>::new());
    }

    // ⑦(补充边界,由 D2.1「文本内容键 = Char 非纯 Ctrl+char + 裸 Enter」导出)
    //    纯 Ctrl+char 不计入 n → 同批裸 Enter 仍 Submit(n=1)
    #[test]
    fn pure_ctrl_char_does_not_count_so_enter_stays_submit() {
        let keys = [
            key_with_modifiers(KeyCode::Char('j'), KeyModifiers::CONTROL),
            key(KeyCode::Enter),
        ];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Passthrough, KeyIntent::Submit]
        );
    }

    // ⑨(review 补)纯裸-Enter 批:裸 Enter 本身计入 n,粘贴 "\n\n" 时 n=2
    //    → 两个 Enter 均换行(「批内有 Char 才换行」的错误实现过不了)
    #[test]
    fn bare_enter_only_burst_classifies_all_as_newline() {
        let keys = [key(KeyCode::Enter), key(KeyCode::Enter)];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Newline, KeyIntent::Newline]
        );
    }

    // ⑧(补充边界,由 D2.1 导出)AltGr(CONTROL|ALT)合成字符是文本内容键、计入 n
    //    → n=2,同批裸 Enter 归换行
    #[test]
    fn altgr_char_counts_as_text_so_enter_becomes_newline() {
        let altgr = key_with_modifiers(
            KeyCode::Char('ä'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let keys = [altgr, key(KeyCode::Enter)];
        assert_eq!(
            classify_key_batch(&keys),
            vec![KeyIntent::Passthrough, KeyIntent::Newline]
        );
    }

    #[test]
    fn would_submit_lone_enter_returns_true_for_enter_press() {
        let batch = [press_event(KeyCode::Enter)];

        assert!(would_submit_lone_enter(&batch));
    }

    #[test]
    fn would_submit_lone_enter_ignores_enter_release() {
        let batch = [press_event(KeyCode::Enter), release_event(KeyCode::Enter)];

        assert!(would_submit_lone_enter(&batch));
    }

    #[test]
    fn would_submit_lone_enter_returns_false_for_char_then_enter() {
        let batch = [press_event(KeyCode::Char('a')), press_event(KeyCode::Enter)];

        assert!(!would_submit_lone_enter(&batch));
    }

    #[test]
    fn would_submit_lone_enter_returns_false_for_char_only() {
        let batch = [press_event(KeyCode::Char('a'))];

        assert!(!would_submit_lone_enter(&batch));
    }

    #[test]
    fn would_submit_lone_enter_returns_false_for_empty_batch() {
        assert!(!would_submit_lone_enter(&[]));
    }

    #[test]
    fn would_submit_lone_enter_returns_false_after_continuation_char_is_merged() {
        let batch = [press_event(KeyCode::Enter), press_event(KeyCode::Char('a'))];

        assert!(!would_submit_lone_enter(&batch));
    }
}
