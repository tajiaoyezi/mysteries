//! 批次分类纯逻辑:滤除非 Press 键事件 + 按"文本内容键数 n"判定裸 Enter 换行/提交。
//! 见 openspec/changes/guard-paste-burst-submit design.md D2。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

pub const PASTE_FOLD_MIN_LINES: usize = 15;
pub const PASTE_FOLD_MIN_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyIntent {
    Newline,
    Submit,
    Passthrough,
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

pub fn fold_candidate(batch: &[Event], min_lines: usize, min_chars: usize) -> Option<String> {
    let keys = press_key_events(batch);
    if keys.is_empty() || !keys.iter().all(is_text_content_key) {
        return None;
    }
    let text: String = keys
        .iter()
        .map(|key| match key.code {
            KeyCode::Enter => '\n',
            KeyCode::Char(ch) => ch,
            _ => unreachable!("is_text_content_key guarantees Char or bare Enter"),
        })
        .collect();
    (text.split('\n').count() >= min_lines || text.chars().count() >= min_chars).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{
        classify_key_batch, fold_candidate, press_key_events, would_submit_lone_enter, KeyIntent,
        PASTE_FOLD_MIN_CHARS, PASTE_FOLD_MIN_LINES,
    };
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
        let expected = std::iter::repeat_n("你", 15)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(result.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn fold_candidate_returns_some_for_single_line_above_char_threshold() {
        let batch = paste_chars_batch(600, 'x');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);

        assert!(result.is_some(), "single-line paste with 600 chars should fold");
    }

    #[test]
    fn fold_candidate_returns_some_for_single_line_at_char_threshold() {
        let batch = paste_chars_batch(PASTE_FOLD_MIN_CHARS, 'x');
        let result = fold_candidate(&batch, PASTE_FOLD_MIN_LINES, PASTE_FOLD_MIN_CHARS);

        assert!(result.is_some(), "single-line paste at 500 chars should fold");
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
