pub mod message;

use crate::agent::message::Message;
use crate::error::ProviderError;
use crate::provider::{DeltaSink, ModelRequest, Provider};

const DEFAULT_SYSTEM_PROMPT: &str = "You are Mysteries, a helpful coding assistant.";
const DEFAULT_MODEL: &str = "mock-model";

pub async fn run_single_turn(
    provider: &dyn Provider,
    prompt: &str,
    sink: &dyn DeltaSink,
) -> Result<String, ProviderError> {
    let response = provider
        .complete(
            ModelRequest {
                model: DEFAULT_MODEL.to_string(),
                messages: vec![
                    Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                    Message::User(prompt.to_string()),
                ],
                max_tokens: None,
            },
            sink,
        )
        .await?;

    Ok(response.text)
}

#[cfg(test)]
mod tests {
    use super::run_single_turn;
    use crate::agent::message::Message;
    use crate::provider::mock::MockProvider;
    use crate::provider::{DeltaSink, FinishReason, ModelResponse};
    use std::sync::Mutex;

    struct CaptureSink {
        chunks: Mutex<Vec<String>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
            }
        }
    }

    impl DeltaSink for CaptureSink {
        fn on_text(&self, text: &str) {
            self.chunks.lock().unwrap().push(text.to_string());
        }
    }

    fn response(text: &str) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
        }
    }

    #[tokio::test]
    async fn run_single_turn_builds_request_returns_text_and_streams_delta() {
        let provider = MockProvider::new(vec![response("model reply")]);
        let sink = CaptureSink::new();

        let text = run_single_turn(&provider, "user prompt", &sink)
            .await
            .unwrap();

        assert_eq!(text, "model reply");
        assert_eq!(*sink.chunks.lock().unwrap(), vec!["model reply"]);

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].messages.len(), 2);
        assert!(matches!(recorded[0].messages[0], Message::System(_)));
        assert_eq!(
            recorded[0].messages[1],
            Message::User("user prompt".to_string())
        );
    }
}
