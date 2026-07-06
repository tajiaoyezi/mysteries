use crate::agent::context::{ContextError, ContextStrategy};
use crate::agent::message::Message;
use crate::provider::{DeltaSink, ModelRequest, Provider, Usage};
use async_trait::async_trait;
use std::sync::Arc;

pub const SUMMARY_HEADER: &str = "\n\n# 此前对话摘要\n";

#[derive(Clone, Debug, PartialEq)]
pub struct CompactionSettings {
    /// 显式窗口覆盖(config `model_context_window`);`None` 时按当前 model 经
    /// 内置表 / 保守默认解析(见 `provider::model_meta`)。
    pub model_context_window: Option<u32>,
    pub compact_trigger_ratio: f32,
    pub keep_recent_turns: u32,
}

pub struct Compacting {
    provider: Arc<dyn Provider>,
    model: String,
    settings: CompactionSettings,
}

struct NoopSink;

impl DeltaSink for NoopSink {
    fn on_text(&self, _text: &str) {}
}

impl Compacting {
    pub fn new(provider: Arc<dyn Provider>, model: String, settings: CompactionSettings) -> Self {
        Self {
            provider,
            model,
            settings,
        }
    }

    pub fn set_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider;
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    /// 手动 `/compact`:无视阈值,立即压缩一次。
    pub async fn compact_now(&self, history: &[Message]) -> Result<Vec<Message>, ContextError> {
        self.compact_history(history, None, true).await
    }

    fn exceeds_threshold(&self, last_usage: Option<&Usage>) -> bool {
        let Some(usage) = last_usage else {
            return false;
        };
        // 有效窗口在判定时按当前 model 解析(显式配置 > 内置表 > 保守默认),
        // 使 /model、/models 运行时切换后窗口自动跟随。
        let window = crate::provider::model_meta::resolve_context_window(
            self.settings.model_context_window,
            &self.model,
        );
        let threshold = window as f32 * self.settings.compact_trigger_ratio;
        usage.input_tokens as f32 > threshold
    }

    fn keep_start_index(history: &[Message], keep_recent_turns: u32) -> Option<usize> {
        if keep_recent_turns == 0 {
            return Some(history.len());
        }

        let mut users_from_end = 0u32;
        for (idx, msg) in history.iter().enumerate().rev() {
            if matches!(msg, Message::User(_)) {
                users_from_end += 1;
                if users_from_end == keep_recent_turns {
                    return Some(idx);
                }
            }
        }

        None
    }

    fn format_message_for_summary(msg: &Message) -> String {
        match msg {
            Message::System(text) => format!("[system]\n{text}"),
            Message::User(text) => format!("[user]\n{text}"),
            Message::Assistant {
                text, tool_calls, ..
            } => {
                let mut part = format!("[assistant]\n{text}");
                for call in tool_calls {
                    part.push_str(&format!("\n[tool_call {} {}]", call.name, call.arguments));
                }
                part
            }
            Message::ToolResult {
                call_id,
                content,
                is_error,
            } => format!("[tool_result {call_id} error={is_error}]\n{content}"),
        }
    }

    fn build_summary_prompt_body(body: &str) -> String {
        format!(
            "请将以下对话历史压缩为结构化摘要,严格按以下分节输出:\n\n\
             ## 已完成工作\n\
             ## 当前文件与代码状态\n\
             ## 关键决策\n\
             ## 下一步待办\n\n\
             对话历史:\n\n{body}"
        )
    }

    /// 将 System 文本拆为「原始 system prompt」与可选「旧 summary」。
    pub fn split_system_text(system_text: &str) -> (String, Option<String>) {
        let Some(idx) = system_text.find(SUMMARY_HEADER) else {
            return (system_text.to_string(), None);
        };

        let original = system_text[..idx].to_string();
        let old_summary = system_text[idx + SUMMARY_HEADER.len()..].to_string();
        (original, Some(old_summary))
    }

    async fn compact_history(
        &self,
        history: &[Message],
        last_usage: Option<&Usage>,
        force: bool,
    ) -> Result<Vec<Message>, ContextError> {
        if !force && !self.exceeds_threshold(last_usage) {
            return Ok(history.to_vec());
        }

        let Some((compress_start, keep_start)) = self.compression_range(history) else {
            return Ok(history.to_vec());
        };

        let Some((_, system_text)) = history.iter().enumerate().find_map(|(i, msg)| {
            if let Message::System(text) = msg {
                Some((i, text.clone()))
            } else {
                None
            }
        }) else {
            return Ok(history.to_vec());
        };

        let (original_system_prompt, old_summary) = Self::split_system_text(&system_text);
        let to_summarize = &history[compress_start..keep_start];

        let mut body_parts = Vec::new();
        if let Some(old) = old_summary.filter(|s| !s.is_empty()) {
            body_parts.push(format!("[previous_summary]\n{old}"));
        }
        body_parts.extend(to_summarize.iter().map(Self::format_message_for_summary));
        let prompt = Self::build_summary_prompt_body(&body_parts.join("\n\n"));

        let summary = match self
            .provider
            .complete(
                ModelRequest {
                    model: self.model.clone(),
                    messages: vec![Message::User(prompt)],
                    tools: Vec::new(),
                    max_tokens: None,
                    thinking: None,
                },
                &NoopSink,
            )
            .await
        {
            Ok(response) => response.text,
            Err(_) => return Ok(history.to_vec()),
        };

        let mut compacted = Vec::with_capacity(1 + history.len().saturating_sub(keep_start));
        compacted.push(Message::System(format!(
            "{original_system_prompt}{SUMMARY_HEADER}{summary}"
        )));
        compacted.extend_from_slice(&history[keep_start..]);
        Ok(compacted)
    }

    fn compression_range(&self, history: &[Message]) -> Option<(usize, usize)> {
        let (system_idx, _) = history.iter().enumerate().find_map(|(i, msg)| {
            if matches!(msg, Message::System(_)) {
                Some((i, ()))
            } else {
                None
            }
        })?;

        let keep_start = Self::keep_start_index(history, self.settings.keep_recent_turns)?;
        let compress_start = system_idx + 1;
        if compress_start >= keep_start {
            return None;
        }

        Some((compress_start, keep_start))
    }

    /// 是否存在可压缩区间(不判定阈值)。
    pub fn can_compress(&self, history: &[Message]) -> bool {
        self.compression_range(history).is_some()
    }
}

#[async_trait]
impl ContextStrategy for Compacting {
    async fn prepare(
        &self,
        history: &[Message],
        last_usage: Option<&Usage>,
    ) -> Result<Vec<Message>, ContextError> {
        self.compact_history(history, last_usage, false).await
    }

    fn set_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider;
    }

    fn set_model(&mut self, model: String) {
        self.model = model;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactCommandOutcome {
    pub notice: String,
    pub changed: bool,
}

/// headless `/compact` 执行:立即压缩当前 history,返回 notice。
pub async fn run_compact_command(
    compacting: &Compacting,
    history: &mut Vec<Message>,
) -> CompactCommandOutcome {
    let original = history.clone();

    match compacting.compact_now(&original).await {
        Ok(compacted) if compacted == original => {
            if compacting.can_compress(&original) {
                CompactCommandOutcome {
                    notice: "压缩失败,请稍后重试 /compact".to_string(),
                    changed: false,
                }
            } else {
                CompactCommandOutcome {
                    notice: "无可压缩内容".to_string(),
                    changed: false,
                }
            }
        }
        Ok(compacted) => {
            *history = compacted;
            CompactCommandOutcome {
                notice: "已压缩上下文".to_string(),
                changed: true,
            }
        }
        Err(err) => CompactCommandOutcome {
            notice: format!("压缩失败:{err},请稍后重试 /compact"),
            changed: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{run_compact_command, Compacting, CompactionSettings, SUMMARY_HEADER};
    use crate::agent::context::ContextStrategy;
    use crate::agent::message::Message;
    use crate::provider::mock::MockProvider;
    use crate::provider::{FinishReason, ModelResponse, ToolCall, Usage};
    use serde_json::json;
    use std::sync::Arc;

    const SUMMARY_TEXT: &str = "MOCK_SUMMARY: completed work and next steps";
    const SUMMARY_A: &str = "SUMMARY_A: first compression";
    const SUMMARY_B: &str = "SUMMARY_B: re-summarized";

    fn summary_response_with(text: &str) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }
    }

    fn default_settings() -> CompactionSettings {
        CompactionSettings {
            model_context_window: Some(100),
            compact_trigger_ratio: 0.8,
            keep_recent_turns: 1,
        }
    }

    fn over_threshold_usage() -> Usage {
        Usage {
            input_tokens: 81,
            output_tokens: 10,
        }
    }

    fn under_threshold_usage() -> Usage {
        Usage {
            input_tokens: 80,
            output_tokens: 10,
        }
    }

    fn multi_turn_history() -> Vec<Message> {
        vec![
            Message::System("system prompt".to_string()),
            Message::User("turn one".to_string()),
            Message::Assistant {
                text: "reply one".to_string(),
                tool_calls: Vec::new(),
                thinking: Vec::new(),
            },
            Message::User("turn two".to_string()),
            Message::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: json!({ "path": "src/main.rs" }),
                }],
                thinking: Vec::new(),
            },
            Message::ToolResult {
                call_id: "call-1".to_string(),
                content: "fn main() {}".to_string(),
                is_error: false,
            },
            Message::User("turn three".to_string()),
            Message::Assistant {
                text: "reply three".to_string(),
                tool_calls: Vec::new(),
                thinking: Vec::new(),
            },
        ]
    }

    fn summary_response() -> ModelResponse {
        ModelResponse {
            text: SUMMARY_TEXT.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        }
    }

    fn compacting_with_provider(provider: Arc<MockProvider>) -> Compacting {
        Compacting::new(provider, "compact-model".to_string(), default_settings())
    }

    #[tokio::test]
    async fn window_follows_current_model_via_builtin_table() {
        // 无显式覆盖:构造于表内大窗口 model(claude 200k),7k tokens 不触发;
        // set_model 切到表内小窗口 model(gpt-4 8k)后,同一 usage 必须触发压缩。
        let provider = Arc::new(MockProvider::new(vec![summary_response_with(SUMMARY_TEXT)]));
        let settings = CompactionSettings {
            model_context_window: None,
            ..default_settings()
        };
        let mut compacting = Compacting::new(provider, "claude-sonnet-4".to_string(), settings);
        let history = multi_turn_history();
        let usage = Usage {
            input_tokens: 7_000,
            output_tokens: 10,
        };

        let unchanged = compacting
            .prepare(&history, Some(&usage))
            .await
            .expect("prepare should succeed");
        assert_eq!(
            unchanged, history,
            "7k input tokens must not trigger under claude's 200k window"
        );

        compacting.set_model("gpt-4".to_string());
        let compacted = compacting
            .prepare(&history, Some(&usage))
            .await
            .expect("prepare should succeed");
        assert_ne!(
            compacted, history,
            "same usage must trigger under gpt-4's 8k window (resolved at check time)"
        );
        assert!(
            matches!(&compacted[0], Message::System(text) if text.contains(SUMMARY_TEXT)),
            "compacted history must carry the summary in System"
        );
    }

    #[tokio::test]
    async fn explicit_override_beats_builtin_table() {
        // 显式 Some(100) 覆盖表值:model 在表内是 200k,阈值仍按 100 算(81 > 80 触发)。
        let provider = Arc::new(MockProvider::new(vec![summary_response_with(SUMMARY_TEXT)]));
        let compacting =
            Compacting::new(provider, "claude-sonnet-4".to_string(), default_settings());
        let history = multi_turn_history();

        let compacted = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");
        assert_ne!(
            compacted, history,
            "explicit 100-token window must trigger at 81 input tokens regardless of table"
        );
    }

    #[tokio::test]
    async fn compacting_rewrites_history_when_over_threshold() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert!(
            prepared.len() < history.len(),
            "compressed history should have fewer messages than input"
        );
        assert_eq!(
            prepared[0],
            Message::System(format!("system prompt{SUMMARY_HEADER}{SUMMARY_TEXT}"))
        );
        assert_eq!(
            prepared[1..],
            history[6..],
            "keep_recent_turns=1 should retain the last full turn from its User message"
        );

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0].tools.is_empty());
        assert_eq!(recorded[0].model, "compact-model");
    }

    #[tokio::test]
    async fn compacting_passthrough_when_last_usage_is_none() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, None)
            .await
            .expect("prepare should succeed");

        assert_eq!(prepared, history);
        assert!(provider.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn compacting_passthrough_when_under_threshold() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&under_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert_eq!(prepared, history);
        assert!(provider.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn compacting_keeps_tool_call_pairs_intact_in_recent_turns() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let settings = CompactionSettings {
            keep_recent_turns: 2,
            ..default_settings()
        };
        let compacting = Compacting::new(provider.clone(), "compact-model".to_string(), settings);
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert!(
            prepared.len() < history.len(),
            "compaction must shrink history before checking kept tool pairs"
        );

        let kept = &prepared[1..];
        assert!(
            kept.windows(3).any(|window| {
                matches!(
                    window,
                    [
                        Message::User(user),
                        Message::Assistant { tool_calls, .. },
                        Message::ToolResult { call_id, .. }
                    ] if user == "turn two"
                        && tool_calls.len() == 1
                        && tool_calls[0].id == "call-1"
                        && call_id == "call-1"
                )
            }),
            "turn two assistant.tool_calls and tool_result must stay paired in kept region"
        );
        assert!(
            !kept.iter().any(|msg| {
                matches!(
                    msg,
                    Message::Assistant { tool_calls, .. }
                        if !tool_calls.is_empty()
                            && !kept.iter().any(|other| matches!(
                                other,
                                Message::ToolResult { call_id, .. } if tool_calls
                                    .iter()
                                    .any(|call| call.id == *call_id)
                            ))
                )
            }),
            "kept region must not contain dangling assistant.tool_calls"
        );
    }

    #[tokio::test]
    async fn compacting_puts_summary_in_system_not_new_message() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert!(
            matches!(&prepared[0], Message::System(text) if text.contains(SUMMARY_TEXT)),
            "summary must be appended to the System message"
        );
        assert!(
            !prepared[1..].iter().any(|msg| match msg {
                Message::User(text) | Message::Assistant { text, .. } => {
                    text.contains(SUMMARY_TEXT)
                }
                _ => false,
            }),
            "summary must not appear as a separate User/Assistant message"
        );
    }

    #[tokio::test]
    async fn compacting_degrades_to_passthrough_when_summary_provider_fails() {
        let provider = Arc::new(MockProvider::new(Vec::new()));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("summary failure should degrade to Ok with original history");

        assert_eq!(
            provider.recorded_requests().len(),
            1,
            "summary provider should be invoked before degrading"
        );
        assert_eq!(prepared, history);
    }

    #[tokio::test]
    async fn compacting_with_keep_zero_summarizes_all_non_system_messages() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let settings = CompactionSettings {
            keep_recent_turns: 0,
            ..default_settings()
        };
        let compacting = Compacting::new(provider.clone(), "compact-model".to_string(), settings);
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert_eq!(prepared.len(), 1);
        assert_eq!(
            prepared[0],
            Message::System(format!("system prompt{SUMMARY_HEADER}{SUMMARY_TEXT}"))
        );
        assert_eq!(provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn compacting_passthrough_when_keep_covers_all_user_turns() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let settings = CompactionSettings {
            keep_recent_turns: 5,
            ..default_settings()
        };
        let compacting = Compacting::new(provider.clone(), "compact-model".to_string(), settings);
        let history = multi_turn_history();

        let prepared = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("prepare should succeed");

        assert_eq!(prepared, history);
        assert!(provider.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn compacting_re_summarizes_instead_of_accumulating_old_summary_in_system() {
        let provider = Arc::new(MockProvider::new(vec![
            summary_response_with(SUMMARY_A),
            summary_response_with(SUMMARY_B),
        ]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let first = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("first compaction should succeed");

        let Message::System(first_system) = &first[0] else {
            panic!("expected System message after first compaction");
        };
        assert_eq!(first_system.matches(SUMMARY_HEADER).count(), 1);
        assert!(first_system.contains(SUMMARY_A));

        let mut extended = first.clone();
        extended.push(Message::User("turn four".to_string()));
        extended.push(Message::Assistant {
            text: "reply four".to_string(),
            tool_calls: Vec::new(),
            thinking: Vec::new(),
        });

        let second = compacting
            .prepare(&extended, Some(&over_threshold_usage()))
            .await
            .expect("second compaction should re-summarize");

        let Message::System(second_system) = &second[0] else {
            panic!("expected System message after second compaction");
        };
        assert_eq!(
            second_system.matches(SUMMARY_HEADER).count(),
            1,
            "System must contain exactly one summary section"
        );
        assert!(
            second_system.contains(SUMMARY_B),
            "second compaction should use fresh summary B"
        );
        assert!(
            !second_system.contains(SUMMARY_A),
            "old summary A must not accumulate in System text"
        );
        assert!(
            second_system.starts_with("system prompt"),
            "original system prompt must be preserved"
        );

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        let second_prompt = match &recorded[1].messages[0] {
            Message::User(text) => text.as_str(),
            other => panic!("expected summary User prompt, got {other:?}"),
        };
        assert!(
            second_prompt.contains(SUMMARY_A),
            "re-summary prompt must include previous summary A"
        );
    }

    #[tokio::test]
    async fn compacting_can_compress_again_after_previous_summary() {
        let provider = Arc::new(MockProvider::new(vec![
            summary_response(),
            summary_response(),
        ]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let first = compacting
            .prepare(&history, Some(&over_threshold_usage()))
            .await
            .expect("first compaction should succeed");

        let mut extended = first.clone();
        extended.push(Message::User("turn four".to_string()));
        extended.push(Message::Assistant {
            text: "reply four".to_string(),
            tool_calls: Vec::new(),
            thinking: Vec::new(),
        });

        let second = compacting
            .prepare(&extended, Some(&over_threshold_usage()))
            .await
            .expect("second compaction should succeed");

        assert!(second.len() < extended.len());
        assert!(
            matches!(&second[0], Message::System(text) if text.contains(SUMMARY_TEXT)),
            "re-compaction should keep summary in System"
        );
        assert_eq!(provider.recorded_requests().len(), 2);
    }

    #[tokio::test]
    async fn compact_now_ignores_threshold() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider.clone());
        let history = multi_turn_history();

        let prepared = compacting
            .compact_now(&history)
            .await
            .expect("force compact should succeed");

        assert!(prepared.len() < history.len());
        assert_eq!(provider.recorded_requests().len(), 1);
    }

    #[tokio::test]
    async fn run_compact_command_replaces_history_with_plain_notice() {
        let provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let compacting = compacting_with_provider(provider);
        let mut history = multi_turn_history();
        let before = history.len();

        let outcome = run_compact_command(&compacting, &mut history).await;

        assert!(outcome.changed, "compact command should replace history");
        assert!(history.len() < before);
        assert_eq!(
            outcome.notice, "已压缩上下文",
            "success notice must be plain, without message counts"
        );
    }

    #[tokio::test]
    async fn run_compact_command_notices_failure_without_changing_history() {
        let provider = Arc::new(MockProvider::new(Vec::new()));
        let compacting = compacting_with_provider(provider);
        let mut history = multi_turn_history();
        let original = history.clone();

        let outcome = run_compact_command(&compacting, &mut history).await;

        assert!(!outcome.changed);
        assert_eq!(history, original);
        assert!(
            outcome.notice.contains("失败") || outcome.notice.contains("重试"),
            "failure notice should mention retry: {}",
            outcome.notice
        );
    }

    #[tokio::test]
    async fn compacting_set_provider_and_set_model_apply_to_compact_now() {
        let old_provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let new_provider = Arc::new(MockProvider::new(vec![summary_response()]));
        let mut compacting = compacting_with_provider(old_provider.clone());
        compacting.set_provider(new_provider.clone());
        compacting.set_model("m2".to_string());

        let _ = compacting
            .compact_now(&multi_turn_history())
            .await
            .expect("compact_now should succeed");

        assert!(old_provider.recorded_requests().is_empty());
        let recorded = new_provider.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].model, "m2");
    }
}
