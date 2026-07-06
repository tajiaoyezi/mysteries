//! 模型上下文窗口元数据:内置表 + 解析链(显式配置 > 内置表 > 保守默认)。
//! 纯逻辑、无 IO;表顺序敏感(更特定条目在前,首个命中生效)。

use crate::provider::Depth;

/// 未知模型的保守默认窗口:取小不取大(估小仅致压缩偏早,估大会致压缩缺席)。
pub const DEFAULT_CONTEXT_WINDOW: u32 = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnthropicThinking {
    None,
    Budget {
        effort: bool,
    },
    Adaptive {
        can_disable: bool,
        max_effort: Depth,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenAiThinking {
    None,
    Effort { max: Depth },
}

/// 桩实现:§2.2 前故意返回错误值,令 2.1 测试保持红灯。
const ANTHROPIC_THINKING_TABLE: &[(&str, AnthropicThinking)] = &[
    (
        "claude-opus-4-8",
        AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::Xhigh,
        },
    ),
    (
        "claude-opus-4-7",
        AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::Xhigh,
        },
    ),
    (
        "claude-sonnet-5",
        AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::Xhigh,
        },
    ),
    (
        "claude-fable-5",
        AnthropicThinking::Adaptive {
            can_disable: false,
            max_effort: Depth::Xhigh,
        },
    ),
    (
        "claude-mythos-5",
        AnthropicThinking::Adaptive {
            can_disable: false,
            max_effort: Depth::Xhigh,
        },
    ),
    (
        "claude-opus-4-6",
        AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::High,
        },
    ),
    (
        "claude-sonnet-4-6",
        AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::High,
        },
    ),
    (
        "claude-opus-4-5",
        AnthropicThinking::Budget { effort: true },
    ),
    (
        "claude-haiku-4-5",
        AnthropicThinking::Budget { effort: false },
    ),
];

const OPENAI_THINKING_TABLE: &[(&str, OpenAiThinking)] = &[
    ("gpt-5", OpenAiThinking::Effort { max: Depth::High }),
    ("o1", OpenAiThinking::Effort { max: Depth::High }),
    ("o3", OpenAiThinking::Effort { max: Depth::High }),
    ("o4", OpenAiThinking::Effort { max: Depth::High }),
];

pub fn anthropic_thinking_capability(model: &str) -> AnthropicThinking {
    let model = model.to_ascii_lowercase();
    if let Some(cap) = ANTHROPIC_THINKING_TABLE
        .iter()
        .find_map(|(pattern, cap)| pattern_matches(&model, pattern).then(|| cap.clone()))
    {
        return cap;
    }
    if model.contains("claude") {
        return AnthropicThinking::Adaptive {
            can_disable: true,
            max_effort: Depth::High,
        };
    }
    AnthropicThinking::None
}

pub fn openai_thinking_capability(model: &str) -> OpenAiThinking {
    let model = model.to_ascii_lowercase();
    OPENAI_THINKING_TABLE
        .iter()
        .find_map(|(pattern, cap)| pattern_matches(&model, pattern).then(|| cap.clone()))
        .unwrap_or(OpenAiThinking::None)
}

/// 内置模型窗口表(pattern, tokens)。顺序敏感:更特定条目在前(gpt-4.1 / gpt-4o /
/// gpt-4-turbo 先于 gpt-4);长 pattern 子串匹配(容忍网关前缀名),≤2 字符的短
/// pattern(o 系)边界匹配防误伤。
const WINDOW_TABLE: &[(&str, u32)] = &[
    ("gpt-4.1", 1_047_576),
    ("gpt-4o", 128_000),
    ("gpt-4-turbo", 128_000),
    ("gpt-5", 400_000),
    ("gpt-4", 8_192),
    ("gpt-3.5", 16_385),
    ("o1", 200_000),
    ("o3", 200_000),
    ("o4", 200_000),
    ("claude", 200_000),
    ("gemini", 1_048_576),
    ("deepseek", 65_536),
    ("qwen", 131_072),
    ("glm", 131_072),
    ("kimi", 131_072),
    ("moonshot", 131_072),
];

/// 按内置表查 model 的 context window(大小写不敏感);未收录返回 None。
pub fn context_window_for(model: &str) -> Option<u32> {
    let model = model.to_ascii_lowercase();
    WINDOW_TABLE
        .iter()
        .find_map(|(pattern, window)| pattern_matches(&model, pattern).then_some(*window))
}

fn pattern_matches(model: &str, pattern: &str) -> bool {
    if pattern.len() > 2 {
        model.contains(pattern)
    } else {
        model == pattern
            || model.starts_with(&format!("{pattern}-"))
            || model.contains(&format!("/{pattern}"))
    }
}

/// 解析有效窗口:显式配置 > 内置表 > `DEFAULT_CONTEXT_WINDOW`。
pub fn resolve_context_window(explicit: Option<u32>, model: &str) -> u32 {
    explicit
        .or_else(|| context_window_for(model))
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::{context_window_for, resolve_context_window, DEFAULT_CONTEXT_WINDOW};
    use crate::provider::Depth;

    #[test]
    fn resolve_prefers_explicit_over_table_over_default() {
        assert_eq!(
            resolve_context_window(Some(50_000), "claude-sonnet-4"),
            50_000,
            "explicit config must beat the builtin table"
        );
        assert_eq!(
            resolve_context_window(None, "claude-sonnet-4"),
            200_000,
            "without explicit config the builtin table applies"
        );
        assert_eq!(
            resolve_context_window(None, "totally-unknown-model"),
            DEFAULT_CONTEXT_WINDOW,
            "unknown model falls back to the conservative default"
        );
    }

    #[test]
    fn table_lookup_is_case_insensitive() {
        assert_eq!(context_window_for("Claude-Sonnet-4"), Some(200_000));
        assert_eq!(context_window_for("GPT-4o"), Some(128_000));
    }

    #[test]
    fn more_specific_entries_shadow_generic_gpt4() {
        assert_eq!(
            context_window_for("gpt-4.1-mini"),
            Some(1_047_576),
            "gpt-4.1 must not be shadowed by the legacy gpt-4 entry"
        );
        assert_eq!(context_window_for("gpt-4o-mini"), Some(128_000));
        assert_eq!(
            context_window_for("gpt-4"),
            Some(8_192),
            "legacy gpt-4 keeps its own small window"
        );
    }

    #[test]
    fn gateway_prefixed_names_match_by_substring() {
        assert_eq!(context_window_for("openai/gpt-4o"), Some(128_000));
        assert_eq!(context_window_for("wps-gpt-4o"), Some(128_000));
    }

    #[test]
    fn short_o_series_patterns_match_on_boundaries_only() {
        assert_eq!(context_window_for("o3-mini"), Some(200_000));
        assert_eq!(context_window_for("openai/o1"), Some(200_000));
        assert_eq!(
            context_window_for("yi-o1-chat"),
            None,
            "o1 must not match as a bare substring inside other names"
        );
    }

    #[test]
    fn unknown_model_returns_none_from_table() {
        assert_eq!(context_window_for("totally-unknown-model"), None);
    }

    #[test]
    fn anthropic_thinking_capability_matches_known_models() {
        use super::{anthropic_thinking_capability, AnthropicThinking};

        assert_eq!(
            anthropic_thinking_capability("claude-opus-4-8"),
            AnthropicThinking::Adaptive {
                can_disable: true,
                max_effort: Depth::Xhigh,
            }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-sonnet-5"),
            AnthropicThinking::Adaptive {
                can_disable: true,
                max_effort: Depth::Xhigh,
            }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-opus-4-6"),
            AnthropicThinking::Adaptive {
                can_disable: true,
                max_effort: Depth::High,
            }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-opus-4-5"),
            AnthropicThinking::Budget { effort: true }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-haiku-4-5"),
            AnthropicThinking::Budget { effort: false }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-fable-5"),
            AnthropicThinking::Adaptive {
                can_disable: false,
                max_effort: Depth::Xhigh,
            }
        );
        assert_eq!(
            anthropic_thinking_capability("claude-future-x"),
            AnthropicThinking::Adaptive {
                can_disable: true,
                max_effort: Depth::High,
            }
        );
    }

    #[test]
    fn openai_thinking_capability_matches_known_models() {
        use super::{openai_thinking_capability, OpenAiThinking};

        assert_eq!(
            openai_thinking_capability("gpt-5"),
            OpenAiThinking::Effort { max: Depth::High }
        );
        assert_eq!(
            openai_thinking_capability("totally-unknown-model"),
            OpenAiThinking::None
        );
    }
}
