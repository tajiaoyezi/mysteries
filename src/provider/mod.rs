use crate::agent::message::Message;
use crate::error::ProviderError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod mock;
pub mod wire;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    Other(String),
}

pub trait DeltaSink: Send + Sync {
    fn on_text(&self, text: &str);
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    async fn complete(
        &self,
        req: ModelRequest,
        sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::{DeltaSink, FinishReason, ModelRequest, ModelResponse, Provider};
    use crate::agent::message::Message;
    use crate::error::ProviderError;
    use async_trait::async_trait;
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

    struct NoopSink;

    impl DeltaSink for NoopSink {
        fn on_text(&self, _text: &str) {}
    }

    struct FakeProvider;

    #[async_trait]
    impl Provider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }

        async fn complete(
            &self,
            req: ModelRequest,
            sink: &dyn DeltaSink,
        ) -> Result<ModelResponse, ProviderError> {
            assert_eq!(
                req.messages,
                vec![Message::User("hello provider".to_string())]
            );

            sink.on_text("hello");
            sink.on_text(" world");

            Ok(ModelResponse {
                text: "hello world".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
            })
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            model: "test-model".to_string(),
            messages: vec![Message::User("hello provider".to_string())],
            max_tokens: Some(64),
        }
    }

    #[tokio::test]
    async fn provider_can_be_called_through_trait_object() {
        let provider: Box<dyn Provider> = Box::new(FakeProvider);
        let sink = NoopSink;

        let response = provider.complete(request(), &sink).await.unwrap();

        assert_eq!(provider.name(), "fake");
        assert_eq!(response.text, "hello world");
        assert_eq!(response.finish_reason, FinishReason::Stop);
    }

    #[tokio::test]
    async fn delta_sink_captures_text_and_noop_sink_drops_it() {
        let provider = FakeProvider;
        let capture = CaptureSink::new();

        let response = provider.complete(request(), &capture).await.unwrap();

        assert_eq!(*capture.chunks.lock().unwrap(), vec!["hello", " world"]);
        assert_eq!(response.text, "hello world");

        let noop = NoopSink;
        let response = provider.complete(request(), &noop).await.unwrap();

        assert_eq!(response.text, "hello world");
    }
}
