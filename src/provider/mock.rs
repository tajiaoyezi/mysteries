use crate::error::ProviderError;
use crate::provider::{DeltaSink, ModelRequest, ModelResponse, Provider};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

pub struct MockProvider {
    script: Vec<ModelResponse>,
    cursor: AtomicUsize,
    recorded: Mutex<Vec<ModelRequest>>,
}

impl MockProvider {
    pub fn new(script: Vec<ModelResponse>) -> Self {
        Self {
            script,
            cursor: AtomicUsize::new(0),
            recorded: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded_requests(&self) -> MutexGuard<'_, Vec<ModelRequest>> {
        self.recorded.lock().unwrap()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn complete(
        &self,
        req: ModelRequest,
        sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        {
            self.recorded.lock().unwrap().push(req);
        }

        let cursor = self.cursor.fetch_add(1, Ordering::SeqCst);
        let Some(response) = self.script.get(cursor).cloned() else {
            return Err(ProviderError::Transport(
                "mock provider script exhausted".to_string(),
            ));
        };

        sink.on_text(&response.text);

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::MockProvider;
    use crate::agent::message::Message;
    use crate::error::ProviderError;
    use crate::provider::{DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider, Usage};
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

    fn request(model: &str, prompt: &str) -> ModelRequest {
        ModelRequest {
            model: model.to_string(),
            messages: vec![Message::User(prompt.to_string())],
            tools: Vec::new(),
            max_tokens: None,
        }
    }

    fn response(text: &str) -> ModelResponse {
        ModelResponse {
            text: text.to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }
    }

    #[tokio::test]
    async fn mock_provider_returns_scripted_responses_in_order_and_records_requests() {
        let provider = MockProvider::new(vec![response("first"), response("second")]);
        let sink = CaptureSink::new();

        let first = provider
            .complete(request("model-a", "prompt-a"), &sink)
            .await
            .unwrap();
        let second = provider
            .complete(request("model-b", "prompt-b"), &sink)
            .await
            .unwrap();

        assert_eq!(first.text, "first");
        assert_eq!(second.text, "second");
        assert_eq!(*sink.chunks.lock().unwrap(), vec!["first", "second"]);

        let recorded = provider.recorded_requests();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].model, "model-a");
        assert_eq!(
            recorded[1].messages,
            vec![Message::User("prompt-b".to_string())]
        );
    }

    #[tokio::test]
    async fn mock_provider_returns_error_when_script_is_exhausted() {
        let provider = MockProvider::new(vec![response("only")]);
        let sink = CaptureSink::new();

        provider
            .complete(request("model", "prompt"), &sink)
            .await
            .unwrap();
        let err = provider
            .complete(request("model", "again"), &sink)
            .await
            .unwrap_err();

        assert!(matches!(err, ProviderError::Transport(_)));
    }

    #[tokio::test]
    async fn mock_provider_preserves_scripted_usage() {
        let usage = Usage {
            input_tokens: 5,
            output_tokens: 8,
        };
        let provider = MockProvider::new(vec![ModelResponse {
            text: "with usage".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: Some(usage.clone()),
        }]);
        let sink = CaptureSink::new();

        let response = provider
            .complete(request("model", "prompt"), &sink)
            .await
            .unwrap();

        assert_eq!(response.usage, Some(usage));
    }
}
