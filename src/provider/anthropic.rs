use crate::credential::CredentialChain;
use crate::error::ProviderError;
use crate::provider::anthropic_stream::AnthropicAccumulator;
use crate::provider::anthropic_wire;
use crate::provider::transport::{
    accumulate_stream, classify, classify_reqwest_error, with_retry, RetryPolicy, TransportFailure,
};
use crate::provider::{DeltaSink, ModelRequest, ModelResponse, Provider};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use serde_json::Value;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_RETRIES: usize = 2;
const DEFAULT_BACKOFF_BASE: Duration = Duration::from_millis(250);
const ANTHROPIC_VERSION: &str = "2023-06-01";
const PROVIDER_LABEL: &str = "Anthropic";

pub struct AnthropicProvider {
    credential_name: String,
    base_url: String,
    credentials: CredentialChain,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
}

impl AnthropicProvider {
    pub fn new(base_url: impl Into<String>, credentials: CredentialChain) -> Self {
        Self::with_credential_name(base_url, credentials, "anthropic")
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
        Self::with_credential_name_and_retry_policy(
            base_url,
            credentials,
            "anthropic",
            retry_policy,
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

    pub fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    pub fn build_request_body(&self, req: &ModelRequest) -> Value {
        let mut body = anthropic_wire::serialize_request(req);
        body["stream"] = Value::Bool(true);
        body
    }

    pub fn attempt_timeout(&self) -> Duration {
        self.retry_policy.attempt_timeout
    }

    pub fn build_authenticated_request(
        &self,
        req: &ModelRequest,
        api_key: &str,
    ) -> Result<reqwest::Request, ProviderError> {
        self.client
            .post(self.messages_url())
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&self.build_request_body(req))
            .build()
            .map_err(|err| {
                ProviderError::Transport(format!("Anthropic request build error: {err}"))
            })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
        let api_key = secret.expose_secret().to_string();
        let url = self.messages_url();
        let body = self.build_request_body(&req);
        let client = self.client.clone();
        let policy = self.retry_policy;

        let response = with_retry(policy, move || {
            let client = client.clone();
            let url = url.clone();
            let body = body.clone();
            let api_key = api_key.clone();

            async move {
                let response = client
                    .post(&url)
                    .header("x-api-key", api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
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
            AnthropicAccumulator::new(),
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
    use super::{AnthropicProvider, ANTHROPIC_VERSION};
    use crate::agent::message::Message;
    use crate::credential::{CredentialChain, CredentialSource, EnvCredentialSource};
    use crate::error::ProviderError;
    use crate::provider::transport::RetryPolicy;
    use crate::provider::{DeltaSink, ModelRequest, Provider, ToolSchema};
    use reqwest::header::{HeaderName, AUTHORIZATION};
    use secrecy::SecretString;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

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
            model: "claude-test".to_string(),
            messages: vec![Message::User("hello".to_string())],
            tools: vec![ToolSchema {
                name: "lookup".to_string(),
                description: "Lookup data".to_string(),
                parameters: json!({ "type": "object" }),
            }],
            max_tokens: Some(128),
            thinking: None,
        }
    }

    #[tokio::test]
    async fn anthropic_provider_returns_auth_when_credentials_are_missing() {
        let provider = AnthropicProvider::with_retry_policy(
            "http://127.0.0.1:9",
            CredentialChain::new(Vec::new()),
            no_retry_policy(),
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_eq!(err, ProviderError::Auth);
        assert_eq!(sink.chunks(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn anthropic_provider_injected_name_rejects_mismatched_chain_key() {
        let provider = AnthropicProvider::with_credential_name(
            "http://127.0.0.1:9",
            CredentialChain::new(vec![Box::new(MapCredentialSource::new(&[(
                "anthropic",
                "sk-anthropic-only",
            )]))]),
            "custom-llm",
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_eq!(err, ProviderError::Auth);
        assert_eq!(sink.chunks(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn anthropic_provider_injected_name_resolves_matching_chain_key_before_http() {
        let provider = AnthropicProvider::with_credential_name(
            "http://127.0.0.1:9",
            CredentialChain::new(vec![Box::new(MapCredentialSource::new(&[(
                "custom-llm",
                "sk-custom",
            )]))]),
            "custom-llm",
        );
        let sink = CaptureSink::new();

        let err = provider.complete(request(), &sink).await.unwrap_err();

        assert_ne!(err, ProviderError::Auth);
    }

    #[test]
    fn anthropic_provider_builds_messages_request_url_headers_and_body_offline() {
        let provider =
            AnthropicProvider::new("http://localhost:11434/", CredentialChain::new(Vec::new()));

        let request = provider
            .build_authenticated_request(&request(), "sk-test")
            .unwrap();

        assert_eq!(
            provider.messages_url(),
            "http://localhost:11434/v1/messages"
        );
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), "http://localhost:11434/v1/messages");
        assert_eq!(
            request
                .headers()
                .get(HeaderName::from_static("x-api-key"))
                .unwrap(),
            "sk-test"
        );
        assert_eq!(
            request
                .headers()
                .get(HeaderName::from_static("anthropic-version"))
                .unwrap(),
            ANTHROPIC_VERSION
        );
        assert!(request.headers().get(AUTHORIZATION).is_none());

        let body_bytes = request.body().and_then(reqwest::Body::as_bytes).unwrap();
        let body: Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(body["model"], json!("claude-test"));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_tokens"], json!(128));
        assert_eq!(body["messages"][0]["role"], json!("user"));
        assert_eq!(
            body["tools"][0]["input_schema"],
            json!({ "type": "object" })
        );
    }

    #[tokio::test]
    #[ignore = "requires ANTHROPIC_API_KEY and network access"]
    async fn anthropic_live_smoke_streams_text() {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            return;
        }

        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-haiku-4-5-20251001".to_string());
        let provider = AnthropicProvider::default(CredentialChain::new(vec![Box::new(
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
