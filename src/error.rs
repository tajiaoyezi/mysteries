use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProviderError {
    #[error("provider authentication failed")]
    Auth,
    #[error("provider rate limited")]
    RateLimited,
    #[error("provider request timed out")]
    Timeout,
    #[error("provider transport error: {0}")]
    Transport(String),
    #[error("provider decode error: {0}")]
    Decode(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("agent loop reached max_iterations limit: {limit}")]
    MaxIterations { limit: u32 },
}
