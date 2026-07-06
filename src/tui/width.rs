pub(crate) fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

pub(crate) fn char_width(ch: char) -> usize {
    if is_zero_width(ch) {
        return 0;
    }

    if matches!(
        ch as u32,
        0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F000..=0x1FAFF
    ) {
        2
    } else {
        1
    }
}

pub(crate) fn truncate_text_to_width(text: &str, max_width: usize) -> String {
    let max_width = max_width.max(1);
    if display_width(text) <= max_width {
        return text.to_string();
    }

    let ellipsis = '…';
    let ellipsis_width = char_width(ellipsis).max(1);
    let content_width = max_width.saturating_sub(ellipsis_width);
    let mut output = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = char_width(ch);
        if ch_width > 0 && width + ch_width > content_width {
            break;
        }
        output.push(ch);
        width += ch_width;
    }
    output.push(ellipsis);
    output
}

fn is_zero_width(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200D | 0xFE00..=0xFE0F
    )
}

#[cfg(test)]
mod tests {
    use super::{char_width, display_width, truncate_text_to_width};

    #[test]
    fn char_width_ascii_and_narrow_latin_is_one() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width(' '), 1);
        assert_eq!(char_width('½'), 1); // U+00BD 拉丁补充,窄
    }

    #[test]
    fn char_width_cjk_and_wide_is_two() {
        assert_eq!(char_width('中'), 2); // CJK 统一表意
        assert_eq!(char_width('あ'), 2); // 平假名
        assert_eq!(char_width('한'), 2); // 韩文音节
        assert_eq!(char_width('，'), 2); // U+FF0C 全角标点
        assert_eq!(char_width('😀'), 2); // U+1F600 emoji
    }

    #[test]
    fn char_width_zero_width_marks_are_zero() {
        assert_eq!(char_width('\u{0301}'), 0); // 组合尖音符
        assert_eq!(char_width('\u{200D}'), 0); // ZWJ
        assert_eq!(char_width('\u{FE0F}'), 0); // variation selector-16
    }

    #[test]
    fn char_width_halfwidth_boundary() {
        // 0xFF00..=0xFF60 为全角(宽 2),0xFF61 起转半角(宽 1)——边界防扩范围
        assert_eq!(char_width('\u{FF60}'), 2);
        assert_eq!(char_width('\u{FF61}'), 1);
        assert_eq!(char_width('\u{FF71}'), 1); // 半角片假名 ｱ
    }

    #[test]
    fn display_width_sums_mixed_text() {
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("a中b"), 4);
        assert_eq!(display_width("e\u{0301}"), 1); // e + 组合符 = 1
    }

    #[test]
    fn truncate_returns_text_when_within_width() {
        assert_eq!(truncate_text_to_width("中文", 4), "中文");
        assert_eq!(truncate_text_to_width("ab", 5), "ab");
    }

    #[test]
    fn truncate_appends_ellipsis_when_over_width() {
        // ascii:content 保留 3 列 + '…'
        assert_eq!(truncate_text_to_width("abcdef", 4), "abc…");
        // cjk:不溢出,'中'(2) + '…'(1) = 3 列
        assert_eq!(truncate_text_to_width("中文字", 4), "中…");
    }

    #[test]
    fn truncate_clamps_zero_max_width_to_one() {
        // max_width 0 → 夹到 1,content 0 → 直接 '…'
        assert_eq!(truncate_text_to_width("abc", 0), "…");
    }
}
