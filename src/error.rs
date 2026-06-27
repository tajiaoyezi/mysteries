use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProviderError {
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
