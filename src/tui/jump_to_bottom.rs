//! 跳到底部 pill 文案与未读助手消息计数(纯逻辑)。

pub fn bump_new_message_count(follows_bottom: bool, current: u32) -> u32 {
    if follows_bottom {
        current
    } else {
        current.saturating_add(1)
    }
}

pub fn new_message_count_on_follow_bottom() -> u32 {
    0
}

pub fn jump_to_bottom_pill_text(count: u32) -> String {
    if count == 0 {
        "跳到底部 (ctrl+End) ↓".to_string()
    } else {
        format!("{count} 条新消息 (ctrl+End) ↓")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bump_new_message_count, jump_to_bottom_pill_text, new_message_count_on_follow_bottom,
    };

    #[test]
    fn bump_increments_when_not_following_bottom() {
        assert_eq!(bump_new_message_count(false, 0), 1);
        assert_eq!(bump_new_message_count(false, 2), 3);
    }

    #[test]
    fn bump_is_noop_when_following_bottom() {
        assert_eq!(bump_new_message_count(true, 0), 0);
        assert_eq!(bump_new_message_count(true, 5), 5);
    }

    #[test]
    fn follow_bottom_clears_count() {
        assert_eq!(new_message_count_on_follow_bottom(), 0);
    }

    #[test]
    fn pill_text_idle_and_with_new_messages() {
        assert_eq!(
            jump_to_bottom_pill_text(0),
            "跳到底部 (ctrl+End) ↓".to_string()
        );
        assert_eq!(
            jump_to_bottom_pill_text(1),
            "1 条新消息 (ctrl+End) ↓".to_string()
        );
        assert_eq!(
            jump_to_bottom_pill_text(3),
            "3 条新消息 (ctrl+End) ↓".to_string()
        );
    }
}
