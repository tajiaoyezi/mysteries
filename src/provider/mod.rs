use crate::agent::message::Message;
use crate::error::ProviderError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

pub mod anthropic;
pub mod anthropic_stream;
pub mod anthropic_wire;
pub mod mock;
pub mod model_meta;
pub mod openai;
pub mod registry;
pub mod stream;
pub mod transport;
pub mod wire;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "lowercase")]
pub enum Depth {
    Off,
    #[default]
    Low,
    Medium,
    High,
    Xhigh,
}

impl Depth {
    pub fn as_effort(&self, cap: Depth) -> &'static str {
        let effective = if *self > cap { cap } else { *self };
        match effective {
            Depth::Off => "off",
            Depth::Low => "low",
            Depth::Medium => "medium",
            Depth::High => "high",
            Depth::Xhigh => "xhigh",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    pub depth: Depth,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub text: String,
    pub signature: Option<String>,
    pub redacted: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Default, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    pub usage: Option<Usage>,
    pub thinking: Vec<ThinkingBlock>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolCalls,
    Other(String),
}

pub trait DeltaSink: Send + Sync {
    fn on_text(&self, text: &str);

    fn on_thinking(&self, _text: &str) {}
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    fn attempt_timeout(&self) -> Option<Duration> {
        None
    }

    async fn complete(
        &self,
        req: ModelRequest,
        sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError>;
}

#[async_trait]
impl<T> Provider for Arc<T>
where
    T: Provider + ?Sized,
{
    fn name(&self) -> &str {
        self.as_ref().name()
    }

    fn attempt_timeout(&self) -> Option<Duration> {
        self.as_ref().attempt_timeout()
    }

    async fn complete(
        &self,
        req: ModelRequest,
        sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        self.as_ref().complete(req, sink).await
    }
}

#[cfg(test)]
mod tests {
    use super::{DeltaSink, Depth, FinishReason, ModelRequest, ModelResponse, Provider, Usage};
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
                usage: None,
                thinking: Vec::new(),
            })
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            model: "test-model".to_string(),
            messages: vec![Message::User("hello provider".to_string())],
            tools: Vec::new(),
            max_tokens: Some(64),
            thinking: None,
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

    #[test]
    fn usage_total_adds_input_and_output_tokens() {
        let response = ModelResponse {
            text: "usage".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
            }),
            thinking: Vec::new(),
        };

        let usage = response.usage.as_ref().expect("usage should be present");
        assert_eq!(usage.total(), 18);
    }

    #[test]
    fn model_response_without_usage_represents_unknown_usage() {
        let response = ModelResponse {
            text: "no usage".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
            thinking: Vec::new(),
        };

        assert_eq!(response.usage, None);
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

    #[test]
    fn depth_serde_roundtrip() {
        let depth = Depth::High;
        let json = serde_json::to_string(&depth).unwrap();
        assert_eq!(json, "\"high\"");
        let parsed: Depth = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Depth::High);
    }

    #[test]
    fn depth_as_effort_caps_xhigh_to_model_max() {
        assert_eq!(Depth::Xhigh.as_effort(Depth::High), "high");
        assert_eq!(Depth::Medium.as_effort(Depth::Xhigh), "medium");
        assert_eq!(Depth::Low.as_effort(Depth::High), "low");
        assert_eq!(Depth::High.as_effort(Depth::High), "high");
        assert_eq!(Depth::Xhigh.as_effort(Depth::Xhigh), "xhigh");
    }

    #[test]
    fn model_request_default_leaves_thinking_empty() {
        let req = ModelRequest {
            model: "test".to_string(),
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
            ..Default::default()
        };
        assert_eq!(req.thinking, None);
    }

    #[test]
    fn model_response_default_leaves_thinking_empty() {
        let response = ModelResponse {
            text: String::new(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            ..Default::default()
        };
        assert!(response.thinking.is_empty());
    }
}
