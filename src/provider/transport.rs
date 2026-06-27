use crate::error::ProviderError;
use crate::provider::{DeltaSink, ModelResponse};
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorClassification {
    Retryable(ProviderError),
    Fatal(ProviderError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub attempt_timeout: Duration,
    pub backoff_base: Duration,
}

impl RetryPolicy {
    pub fn new(max_retries: usize, attempt_timeout: Duration, backoff_base: Duration) -> Self {
        Self {
            max_retries,
            attempt_timeout,
            backoff_base,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErrorKind {
    Timeout,
    Network,
    Decode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportFailure {
    Status(u16),
    Error(TransportErrorKind),
}

pub trait SseAccumulator {
    fn push_chunk(
        &mut self,
        chunk: &[u8],
        sink: &dyn DeltaSink,
    ) -> Result<Option<ModelResponse>, ProviderError>;

    fn finish(&self) -> Result<ModelResponse, ProviderError>;
}

pub fn classify(failure: TransportFailure, provider_label: &str) -> ErrorClassification {
    match failure {
        TransportFailure::Status(401 | 403) => ErrorClassification::Fatal(ProviderError::Auth),
        TransportFailure::Status(429) => ErrorClassification::Retryable(ProviderError::RateLimited),
        TransportFailure::Status(status) if (500..=599).contains(&status) => {
            ErrorClassification::Retryable(ProviderError::RateLimited)
        }
        TransportFailure::Status(status) => ErrorClassification::Fatal(ProviderError::Transport(
            format!("{provider_label} HTTP status {status}"),
        )),
        TransportFailure::Error(TransportErrorKind::Timeout) => {
            ErrorClassification::Retryable(ProviderError::Timeout)
        }
        TransportFailure::Error(TransportErrorKind::Network) => ErrorClassification::Retryable(
            ProviderError::Transport(format!("{provider_label} network error")),
        ),
        TransportFailure::Error(TransportErrorKind::Decode) => ErrorClassification::Fatal(
            ProviderError::Decode(format!("{provider_label} response decode error")),
        ),
    }
}

pub fn classify_reqwest_error(err: &reqwest::Error, provider_label: &str) -> ErrorClassification {
    if err.is_timeout() {
        classify(
            TransportFailure::Error(TransportErrorKind::Timeout),
            provider_label,
        )
    } else if err.is_decode() {
        classify(
            TransportFailure::Error(TransportErrorKind::Decode),
            provider_label,
        )
    } else {
        classify(
            TransportFailure::Error(TransportErrorKind::Network),
            provider_label,
        )
    }
}

pub async fn with_retry<T, F, Fut>(policy: RetryPolicy, attempt: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ErrorClassification>>,
{
    let mut attempt = attempt;
    let mut retries_used = 0;

    loop {
        let outcome = tokio::time::timeout(policy.attempt_timeout, attempt()).await;
        let classification = match outcome {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(classification)) => classification,
            Err(_) => ErrorClassification::Retryable(ProviderError::Timeout),
        };

        match classification {
            ErrorClassification::Fatal(error) => return Err(error),
            ErrorClassification::Retryable(error) => {
                if retries_used >= policy.max_retries {
                    return Err(error);
                }

                tokio::time::sleep(backoff_delay(policy.backoff_base, retries_used)).await;
                retries_used += 1;
            }
        }
    }
}

pub fn backoff_delay(base: Duration, retries_used: usize) -> Duration {
    let factor = 1_u32.checked_shl(retries_used as u32).unwrap_or(u32::MAX);
    base.saturating_mul(factor)
}

pub async fn accumulate_stream<S, B, E, A>(
    stream: S,
    sink: &dyn DeltaSink,
    mut accumulator: A,
    provider_label: &str,
) -> Result<ModelResponse, ProviderError>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
    A: SseAccumulator,
{
    futures_util::pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| {
            ProviderError::Transport(format!("{provider_label} stream error: {err}"))
        })?;
        if let Some(response) = accumulator.push_chunk(chunk.as_ref(), sink)? {
            return Ok(response);
        }
    }

    accumulator.finish()
}
