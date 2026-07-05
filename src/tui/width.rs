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
