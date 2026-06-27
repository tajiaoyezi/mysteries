use crate::agent::message::Message;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::app::{assemble_agent, load_config, select_provider, AssemblyError};
use crate::credential::{CredentialChain, EnvCredentialSource, FileCredentialSource};
use crate::error::AgentError;
use crate::permission::{PermissionDecider, PermissionDecision};
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::Tool;
use crate::tool::ToolContext;
use async_trait::async_trait;
use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;
use tokio::task;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliPaths {
    pub user_config: PathBuf,
    pub project_config: PathBuf,
    pub credentials: PathBuf,
    pub cwd: PathBuf,
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Assembly(#[from] AssemblyError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("io error: {0}")]
    Io(String),
}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

pub struct StdinDecider;

pub struct StdoutSink;

impl DeltaSink for StdoutSink {
    fn on_text(&self, text: &str) {
        print!("{text}");
        let _ = io::stdout().flush();
    }
}

pub fn parse_decision(input: &str) -> PermissionDecision {
    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => PermissionDecision::Allow,
        _ => PermissionDecision::Deny,
    }
}

#[async_trait]
impl PermissionDecider for StdinDecider {
    async fn decide(&self, call: &ToolCall, tool: &dyn Tool) -> PermissionDecision {
        eprintln!("tool requires confirmation: {}", tool.name());
        eprintln!("arguments: {}", call.arguments);
        eprint!("allow? [y/n] ");
        let _ = io::stderr().flush();

        let input = task::spawn_blocking(read_stdin_line).await;
        match input {
            Ok(Some(input)) => parse_decision(&input),
            Ok(None) | Err(_) => PermissionDecision::Deny,
        }
    }
}

pub async fn run_cli(paths: CliPaths, prompt: &str) -> Result<(), CliError> {
    let config = load_config(&paths.user_config, &paths.project_config)?;
    let credentials = CredentialChain::new(vec![
        Box::new(EnvCredentialSource::new()),
        Box::new(FileCredentialSource::new(paths.credentials.clone())),
    ]);
    let provider = select_provider(&config, credentials)?;
    let agent = assemble_agent(provider, &config, Box::new(StdinDecider));
    let ctx = ToolContext {
        cwd: paths.cwd,
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
    };
    let sink = StdoutSink;
    let mut history = initial_history(prompt);

    agent.run(&mut history, &ctx, &sink).await?;
    println!();

    Ok(())
}

fn read_stdin_line() -> Option<String> {
    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(input),
    }
}

fn initial_history(prompt: &str) -> Vec<Message> {
    vec![
        Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
        Message::User(prompt.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::{initial_history, parse_decision};
    use crate::agent::message::Message;
    use crate::agent::DEFAULT_SYSTEM_PROMPT;
    use crate::permission::PermissionDecision;

    #[test]
    fn parse_decision_allows_y_yes_case_insensitive_and_trimmed() {
        for input in ["y", "Y", "yes", " YES ", "\ty\n"] {
            assert_eq!(parse_decision(input), PermissionDecision::Allow);
        }
    }

    #[test]
    fn parse_decision_denies_non_confirmation_empty_and_eof_equivalent() {
        for input in ["n", "", "   ", "maybe", "no"] {
            assert_eq!(parse_decision(input), PermissionDecision::Deny);
        }
    }

    #[test]
    fn initial_history_seeds_system_then_user_prompt() {
        assert_eq!(
            initial_history("hello"),
            vec![
                Message::System(DEFAULT_SYSTEM_PROMPT.to_string()),
                Message::User("hello".to_string()),
            ]
        );
    }
}
