use crate::agent::message::Message;
use crate::provider::Usage;
use async_trait::async_trait;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum ContextError {
    PrepareFailed(String),
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrepareFailed(msg) => write!(f, "context prepare failed: {msg}"),
        }
    }
}

impl std::error::Error for ContextError {}

#[async_trait]
pub trait ContextStrategy: Send + Sync {
    async fn prepare(
        &self,
        history: &[Message],
        last_usage: Option<&Usage>,
    ) -> Result<Vec<Message>, ContextError>;
}

/// 默认策略：原样返回 history 克隆。
pub struct Passthrough;

#[async_trait]
impl ContextStrategy for Passthrough {
    async fn prepare(
        &self,
        history: &[Message],
        _last_usage: Option<&Usage>,
    ) -> Result<Vec<Message>, ContextError> {
        Ok(history.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextStrategy, Passthrough};
    use crate::agent::message::Message;
    use crate::provider::{ToolCall, Usage};
    use serde_json::json;

    fn sample_history() -> Vec<Message> {
        vec![
            Message::System("system prompt".to_string()),
            Message::User("user turn".to_string()),
            Message::Assistant {
                text: "assistant reply".to_string(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    arguments: json!({ "path": "src/main.rs" }),
                }],
            },
            Message::ToolResult {
                call_id: "call-1".to_string(),
                content: "fn main() {}".to_string(),
                is_error: false,
            },
        ]
    }

    #[tokio::test]
    async fn passthrough_prepare_returns_history_unchanged() {
        let history = sample_history();
        let prepared = Passthrough
            .prepare(&history, None)
            .await
            .expect("Passthrough prepare should succeed");

        assert_eq!(
            prepared, history,
            "Passthrough must return messages identical to input history"
        );
    }

    #[tokio::test]
    async fn passthrough_prepare_ignores_last_usage() {
        let history = sample_history();
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
        };

        let with_usage = Passthrough
            .prepare(&history, Some(&usage))
            .await
            .expect("Passthrough prepare should succeed");
        let without_usage = Passthrough
            .prepare(&history, None)
            .await
            .expect("Passthrough prepare should succeed");

        assert_eq!(with_usage, history);
        assert_eq!(without_usage, history);
        assert_eq!(with_usage, without_usage);
    }
}
