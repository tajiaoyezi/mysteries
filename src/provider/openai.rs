use crate::credential::CredentialChain;
use crate::error::ProviderError;
use crate::provider::stream::StreamAccumulator;
use crate::provider::transport::{
    accumulate_stream, classify, classify_reqwest_error, with_retry, RetryPolicy, TransportFailure,
};
use crate::provider::{wire, DeltaSink, ModelRequest, ModelResponse, Provider};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use serde_json::Value;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RETRIES: usize = 2;
const DEFAULT_BACKOFF_BASE: Duration = Duration::from_millis(250);
const PROVIDER_LABEL: &str = "OpenAI";

pub struct OpenAiProvider {
    credential_name: String,
    base_url: String,
    credentials: CredentialChain,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
}

impl OpenAiProvider {
    pub fn new(base_url: impl Into<String>, credentials: CredentialChain) -> Self {
        Self::with_credential_name(base_url, credentials, "openai")
    }

    pub fn with_credential_name(
        base_url: impl Into<String>,
        credentials: CredentialChain,
        credential_name: impl Into<String>,
    ) -> Self {
        Self::with_credential_name_and_retry_policy(
            base_url,
            credentials,
            credential_name,
            default_retry_policy(),
        )
    }

    fn with_credential_name_and_retry_policy(
        base_url: impl Into<String>,
        credentials: CredentialChain,
        credential_name: impl Into<String>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            credential_name: credential_name.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            credentials,
            client: reqwest::Client::new(),
            retry_policy,
        }
    }

    pub fn default(credentials: CredentialChain) -> Self {
        Self::new(DEFAULT_BASE_URL, credentials)
    }

    pub fn with_attempt_timeout(
        base_url: impl Into<String>,
        credentials: CredentialChain,
        credential_name: impl Into<String>,
        attempt_timeout: Duration,
    ) -> Self {
        Self::with_credential_name_and_retry_policy(
            base_url,
            credentials,
            credential_name,
            retry_policy_with_attempt_timeout(attempt_timeout),
        )
    }

    pub fn default_with_attempt_timeout(
        credentials: CredentialChain,
        credential_name: impl Into<String>,
        attempt_timeout: Duration,
    ) -> Self {
        Self::with_attempt_timeout(
            DEFAULT_BASE_URL,
            credentials,
            credential_name,
            attempt_timeout,
        )
    }

    pub fn with_retry_policy(
        base_url: impl Into<String>,
        credentials: CredentialChain,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self::with_credential_name_and_retry_policy(base_url, credentials, "openai", retry_policy)
    }

    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    pub fn build_request_body(&self, req: &ModelRequest) -> Value {
        let mut body = wire::serialize_request(req);
        body["stream"] = Value::Bool(true);
        body["stream_options"] = serde_json::json!({ "include_usage": true });
        body
    }

    pub fn attempt_timeout(&self) -> Duration {
        self.retry_policy.attempt_timeout
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn attempt_timeout(&self) -> Option<Duration> {
        Some(self.attempt_timeout())
    }

    async fn complete(
        &self,
        req: ModelRequest,
        sink: &dyn DeltaSink,
    ) -> Result<ModelResponse, ProviderError> {
        let secret = self
            .credentials
            .resolve(&self.credential_name)
            .ok_or(ProviderError::Auth)?;
        let authorization = format!("Bearer {}", secret.expose_secret());
        let url = self.chat_completions_url();
        let body = self.build_request_body(&req);
        let client = self.client.clone();
        let policy = self.retry_policy;

        let response = with_retry(policy, move || {
            let client = client.clone();
            let url = url.clone();
            let body = body.clone();
            let authorization = authorization.clone();

            async move {
                let response = client
                    .post(&url)
                    .header(reqwest::header::AUTHORIZATION, authorization)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|err| classify_reqwest_error(&err, PROVIDER_LABEL))?;

                if !response.status().is_success() {
                    return Err(classify(
                        TransportFailure::Status(response.status().as_u16()),
                        PROVIDER_LABEL,
                    ));
                }

                Ok(response)
            }
        })
        .await?;

        accumulate_stream(
            response.bytes_stream(),
            sink,
            StreamAccumulator::new(),
            PROVIDER_LABEL,
        )
        .await
    }
}

fn default_retry_policy() -> RetryPolicy {
    retry_policy_with_attempt_timeout(DEFAULT_ATTEMPT_TIMEOUT)
}

fn retry_policy_with_attempt_timeout(attempt_timeout: Duration) -> RetryPolicy {
    RetryPolicy::new(DEFAULT_MAX_RETRIES, attempt_timeout, DEFAULT_BACKOFF_BASE)
}

#[cfg(test)]
mod tests {
    use super::{OpenAiProvider, PROVIDER_LABEL};
    use crate::agent::message::Message;
    use crate::credential::{CredentialChain, CredentialSource, EnvCredentialSource};
    use crate::error::ProviderError;
    use crate::provider::stream::StreamAccumulator;
    use crate::provider::transport::{
        accumulate_stream, classify, with_retry, ErrorClassification, RetryPolicy,
        TransportErrorKind, TransportFailure,
    };
    use crate::provider::{DeltaSink, ModelRequest, Provider, ToolCall, ToolSchema};
    use secrecy::SecretString;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::{self, Instant};

    struct MapCredentialSource {
        keys: HashMap<String, String>,
    }

    impl MapCredentialSource {
        fn new(entries: &[(&str, &str)]) -> Self {
            Self {
                keys: entries
                    .iter()
                    .map(|(provider, key)| ((*provider).to_string(), (*key).to_string()))
                    .collect(),
            }
        }
    }

    impl CredentialSource for MapCredentialSource {
        fn resolve(&self, provider: &str) -> Option<SecretString> {
            self.keys
                .get(provider)
                .map(|key| SecretString::from(key.clone()))
        }
    }

    fn no_retry_policy() -> RetryPolicy {
        RetryPolicy::new(0, Duration::from_millis(10), Duration::from_millis(1))
    }

    fn test_policy(max_retries: usize) -> RetryPolicy {
        RetryPolicy::new(
            max_retries,
            Duration::from_secs(5),
            Duration::from_millis(100),
        )
    }

    struct CaptureSink {
        chunks: Mutex<Vec<String>>,
    }

    impl CaptureSink {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
            }
        }

        fn chunks(&self) -> Vec<String> {
            self.chunks.lock().unwrap().clone()
        }
    }

    impl DeltaSink for CaptureSink {
        fn on_text(&self, text: &str) {
            self.chunks.lock().unwrap().push(text.to_string());
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            model: "gpt-test".to_string(),
            messages: vec![
                Message::System("system".to_string()),
                Message::User("hello".to_string()),
                Message::Assistant {
                    text: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: json!({ "query": "rust" }),
                    }],
                    thinking: Vec::new(),
                },
            ],
            tools: vec![ToolSchema {
                name: "lookup".to_string(),
                description: "Lookup data".to_string(),
                parameters: json!({ "type": "object" }),
            }],
            max_tokens: Some(128),
            thinking: None,
        }
    }

    #[test]
    fn classify_401_as_fatal_auth_and_403_as_forbidden_transport() {
        assert_eq!(
            classify(TransportFailure::Status(401), PROVIDER_LABEL),
            ErrorClassification::Fatal(ProviderError::Auth)
        );
        match classify(TransportFailure::Status(403), PROVIDER_LABEL) {
            ErrorClassification::Fatal(ProviderError::Transport(message)) => {
                assert!(message.contains("forbidden (403)"));
                assert!(message.contains("模型无权限或配额"));
                assert!(!message.contains("authentication"));
            }
            other => panic!("403 should be fatal transport, got {other:?}"),
        }
    }

    #[test]
    fn classify_retryable_statuses_as_rate_limited() {
        assert_eq!(
            classify(TransportFailure::Status(429), PROVIDER_LABEL),
            ErrorClassification::Retryable(ProviderError::RateLimited)
        );
        assert_eq!(
            classify(TransportFailure::Status(500), PROVIDER_LABEL),
            ErrorClassification::Retryable(ProviderError::RateLimited)
        );
        assert_eq!(
            classify(TransportFailure::Status(503), PROVIDER_LABEL),
            ErrorClassification::Retryable(ProviderError::RateLimited)
        );
    }

    #[test]
    fn classify_non_retryable_client_statuses_as_fatal_transport() {
        for status in [400, 404] {
            match classify(TransportFailure::Status(status), PROVIDER_LABEL) {
                ErrorClassification::Fatal(ProviderError::Transport(message)) => {
                    assert_eq!(message, format!("OpenAI HTTP status {status}"));
                }
                other => panic!("expected fatal transport for {status}, got {other:?}"),
            }
        }
    }

    #[test]
    fn classify_transport_error_kinds() {
        assert_eq!(
            classify(
                TransportFailure::Error(TransportErrorKind::Timeout),
                PROVIDER_LABEL
            ),
            ErrorClassification::Retryable(ProviderError::Timeout)
        );

        match classify(
            TransportFailure::Error(TransportErrorKind::Network),
            PROVIDER_LABEL,
        ) {
            ErrorClassification::Retryable(ProviderError::Transport(message)) => {
                assert_eq!(message, "OpenAI network error");
            }
            other => panic!("expected retryable network transport, got {other:?}"),
        }

        match classify(
            TransportFailure::Error(TransportErrorKind::Decode),
            PROVIDER_LABEL,
        ) {
            ErrorClassification::Fatal(ProviderError::Decode(message)) => {
                assert_eq!(message, "OpenAI response decode error");
            }
            other => panic!("expected fatal decode, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn with_retry_retries_retryable_errors_then_succeeds() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let started = Instant::now();
        let result = with_retry(test_policy(3), {
            let attempts = attempts.clone();
            move || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                async move {
                    if attempt < 3 {
                        Err(ErrorClassification::Retryable(ProviderError::RateLimited))
                    } else {
                        Ok("ok")
                    }
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(result, "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(Instant::now() - started, Duration::from_millis(300));
    }

    #[tokio::test(start_paused = true)]
    async fn with_retry_does_not_retry_fatal_auth() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let err: ProviderError = with_retry(test_policy(3), {
            let attempts = attempts.clone();
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                async { Err::<&str, _>(ErrorClassification::Fatal(ProviderError::Auth)) }
            }
        })
        .await
        .unwrap_err();

        assert_eq!(err, ProviderError::Auth);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn with_retry_returns_last_error_after_exhausting_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let err: ProviderError = with_retry(test_policy(2), {
            let attempts = attempts.clone();
            move || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                async move {
                    if attempt < 3 {
                        Err::<&str, _>(ErrorClassification::Retryable(ProviderError::RateLimited))
                    } else {
                        Err::<&str, _>(ErrorClassification::Retryable(ProviderError::Timeout))
                    }
                }
            }
        })
        .await
        .unwrap_err();

        assert_eq!(err, ProviderError::Timeout);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn with_retry_times_out_single_attempt_and_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let err: ProviderError = with_retry(
            RetryPolicy::new(1, Duration::from_secs(1), Duration::from_millis(100)),
            {
                let attempts = attempts.clone();
                move || {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    async {
                        time::sleep(Duration::from_secs(10)).await;
                        Ok::<_, ErrorClassification>("late")
                    }
                }
            },
        )
        .await
        .unwrap_err();

        assert_eq!(err, ProviderError::Timeout);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn openai_provider_builds_chat_completions_request_body_and_url() {
        let provider = OpenAiProvider::new(
            "http://localhost:11434/v1/",
            CredentialChain::new(Vec::new()),
        );
        let req = request();

        let body = provider.build_request_body(&req);

        assert_eq!(
            provider.chat_completions_url(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(body["model"], json!("gpt-test"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["stream_options"]["include_usage"], json!(true));
        assert_eq!(body["max_tokens"], json!(128));
        assert_eq!(
            body["messages"][0],
            json!({ "role": "system", "content": "system" })
        );
        assert_eq!(
            body["messages"][1],
            json!({ "role": "user", "content": "hello" })
        );
        assert_eq!(body["tools"][0]["function"]["name"], json!("lookup"));
    }

    #[tokio::test]
    async fn openai_provider_returns_auth_when_credentials_are_missing() {
        let provider = OpenAiProvider::with_retry_policy(
            "http://127.0.0.1:9/v1",
            CredentialChain::new(Vec::new()),
            no_retry_policy(),
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_eq!(err, ProviderError::Auth);
        assert_eq!(sink.chunks(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn openai_provider_injected_name_rejects_mismatched_chain_key() {
        let provider = OpenAiProvider::with_credential_name(
            "http://127.0.0.1:9/v1",
            CredentialChain::new(vec![Box::new(MapCredentialSource::new(&[(
                "openai",
                "sk-openai-only",
            )]))]),
            "deepseek",
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_eq!(err, ProviderError::Auth);
        assert_eq!(sink.chunks(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn openai_provider_injected_name_resolves_matching_chain_key_before_http() {
        let provider = OpenAiProvider::with_credential_name(
            "http://127.0.0.1:9/v1",
            CredentialChain::new(vec![Box::new(MapCredentialSource::new(&[(
                "deepseek",
                "sk-deepseek",
            )]))]),
            "deepseek",
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_ne!(err, ProviderError::Auth);
    }

    #[tokio::test]
    async fn accumulate_stream_returns_stream_error_without_reemitting_text() {
        let sink = CaptureSink::new();
        let stream = futures_util::stream::iter([
            Ok::<_, &'static str>(
                br#"data: {"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}

"#
                .as_slice(),
            ),
            Err("connection reset"),
        ]);

        let err = accumulate_stream(stream, &sink, StreamAccumulator::new(), PROVIDER_LABEL)
            .await
            .unwrap_err();

        assert!(
            matches!(err, ProviderError::Transport(message) if message == "OpenAI stream error: connection reset")
        );
        assert_eq!(sink.chunks(), vec!["partial"]);
    }

    #[tokio::test]
    #[ignore = "requires OPENAI_API_KEY and network access"]
    async fn openai_live_smoke_streams_text() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            return;
        }

        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
        let provider = OpenAiProvider::default(CredentialChain::new(vec![Box::new(
            EnvCredentialSource::new(),
        )]));
        let sink = CaptureSink::new();
        let response = provider
            .complete(
                ModelRequest {
                    model,
                    messages: vec![Message::User("Reply with exactly: pong".to_string())],
                    tools: Vec::new(),
                    max_tokens: Some(16),
                thinking: None,
                },
                &sink,
            )
            .await
            .unwrap();

        assert!(!response.text.trim().is_empty());
        assert!(!sink.chunks().is_empty());
    }
}
