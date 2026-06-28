use crate::agent::message::Message;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::app::{assemble_agent, load_config, select_provider, AssemblyError};
use crate::config::{write_config, ConfigError, ConfigWritePatch, ProviderKind};
use crate::credential::{
    write_credential, CredentialChain, CredentialError, EnvCredentialSource, FileCredentialSource,
};
use crate::error::AgentError;
use crate::permission::{PermissionDecider, PermissionDecision};
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::Tool;
use crate::tool::ToolContext;
use async_trait::async_trait;
use secrecy::SecretString;
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
    #[error(transparent)]
    Auth(#[from] AuthError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("auth cancelled")]
    Cancelled,
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Credential(#[from] CredentialError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthPaths {
    pub user_config: PathBuf,
    pub credentials: PathBuf,
}

pub trait AuthPrompter {
    fn read_line(&mut self, prompt: &str) -> Result<Option<String>, AuthError>;
    fn read_secret(&mut self, prompt: &str) -> Result<Option<SecretString>, AuthError>;
}

pub fn run_auth(paths: &AuthPaths, prompter: &mut dyn AuthPrompter) -> Result<(), AuthError> {
    let provider_line = prompter.read_line("Provider [openai/anthropic]: ")?;
    let provider_line = provider_line.ok_or(AuthError::Cancelled)?;
    let (provider_kind, credential_provider) = parse_auth_provider(&provider_line)?;

    let base_url_line = prompter.read_line("Base URL (empty for default): ")?;
    let base_url_line = base_url_line.ok_or(AuthError::Cancelled)?;
    let base_url = normalize_optional_line(&base_url_line);

    let model_line = prompter.read_line("Model: ")?;
    let model_line = model_line.ok_or(AuthError::Cancelled)?;
    let model = model_line.trim();
    if model.is_empty() {
        return Err(AuthError::Cancelled);
    }

    let key = prompter.read_secret("API key: ")?;
    let key = key.ok_or(AuthError::Cancelled)?;

    write_config(
        &paths.user_config,
        &ConfigWritePatch {
            provider_kind,
            base_url,
            model: model.to_string(),
        },
    )?;

    write_credential(&paths.credentials, credential_provider, &key)?;

    Ok(())
}

pub fn run_auth_interactive(paths: &AuthPaths) -> Result<(), AuthError> {
    let mut prompter = StdinAuthPrompter;
    run_auth(paths, &mut prompter)
}

fn parse_auth_provider(input: &str) -> Result<(ProviderKind, &'static str), AuthError> {
    match input.trim().to_ascii_lowercase().as_str() {
        "openai" => Ok((ProviderKind::OpenAi, "openai")),
        "anthropic" => Ok((ProviderKind::Anthropic, "anthropic")),
        other => Err(AuthError::UnsupportedProvider(other.to_string())),
    }
}

fn normalize_optional_line(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn trim_line_endings(mut input: String) -> String {
    while input.ends_with('\n') || input.ends_with('\r') {
        input.pop();
    }
    input
}

pub struct StdinAuthPrompter;

impl AuthPrompter for StdinAuthPrompter {
    fn read_line(&mut self, prompt: &str) -> Result<Option<String>, AuthError> {
        eprint!("{prompt}");
        let _ = io::stderr().flush();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(trim_line_endings(input))),
            Err(_) => Ok(None),
        }
    }

    fn read_secret(&mut self, prompt: &str) -> Result<Option<SecretString>, AuthError> {
        read_secret_hidden(prompt)
    }
}

fn read_secret_hidden(prompt: &str) -> Result<Option<SecretString>, AuthError> {
    use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    eprint!("{prompt}");
    let _ = io::stderr().flush();

    enable_raw_mode().map_err(|_| AuthError::Cancelled)?;
    let read_result = (|| -> Result<Option<SecretString>, AuthError> {
        let mut secret = String::new();
        loop {
            let event = read().map_err(|_| AuthError::Cancelled)?;
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event
            {
                match code {
                    KeyCode::Enter => break,
                    KeyCode::Esc => return Ok(None),
                    KeyCode::Char('\x03') => return Ok(None),
                    KeyCode::Char(_) if modifiers.contains(KeyModifiers::CONTROL) => {}
                    KeyCode::Char(c) => secret.push(c),
                    KeyCode::Backspace | KeyCode::Delete => {
                        secret.pop();
                    }
                    _ => {}
                }
            }
        }
        if secret.is_empty() {
            Ok(None)
        } else {
            Ok(Some(SecretString::from(secret)))
        }
    })();
    let _ = disable_raw_mode();
    eprintln!();
    read_result
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
    let assembled = assemble_agent(provider, &config, Box::new(StdinDecider));
    let ctx = ToolContext {
        cwd: paths.cwd,
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
    };
    let sink = StdoutSink;
    let mut history = initial_history(prompt);

    assembled.agent.run(&mut history, &ctx, &sink).await?;
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
    use super::{initial_history, parse_decision, run_auth, AuthError, AuthPaths, AuthPrompter};
    use crate::agent::message::Message;
    use crate::agent::DEFAULT_SYSTEM_PROMPT;
    use crate::config::{parse, ProviderKind};
    use crate::credential::{CredentialSource, FileCredentialSource};
    use crate::permission::PermissionDecision;
    use secrecy::{ExposeSecret, SecretString};
    use std::fs;

    struct ScriptedAuthPrompter {
        lines: Vec<Option<String>>,
        secrets: Vec<Option<String>>,
        line_idx: usize,
        secret_idx: usize,
    }

    impl ScriptedAuthPrompter {
        fn new(lines: Vec<Option<String>>, secrets: Vec<Option<String>>) -> Self {
            Self {
                lines,
                secrets,
                line_idx: 0,
                secret_idx: 0,
            }
        }
    }

    impl AuthPrompter for ScriptedAuthPrompter {
        fn read_line(&mut self, _prompt: &str) -> Result<Option<String>, AuthError> {
            let value = self.lines.get(self.line_idx).cloned().unwrap_or(None);
            self.line_idx += 1;
            Ok(value)
        }

        fn read_secret(&mut self, _prompt: &str) -> Result<Option<SecretString>, AuthError> {
            let value = self
                .secrets
                .get(self.secret_idx)
                .and_then(|line| line.as_ref().map(|key| SecretString::from(key.clone())));
            self.secret_idx += 1;
            Ok(value)
        }
    }

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

    #[test]
    fn run_auth_writes_config_and_credentials_with_injected_input() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "old-model"
max_iterations = 40

[provider]
kind = "anthropic"
auth_type = "api_key"
"#,
        )
        .unwrap();
        let credentials_path = temp.path().join("credentials");

        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(
            vec![
                Some("openai".to_string()),
                Some(String::new()),
                Some("gpt-4o".to_string()),
            ],
            vec![Some("sk-xxx".to_string())],
        );

        run_auth(&paths, &mut prompter).unwrap();

        let raw = parse(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("gpt-4o"));
        assert_eq!(raw.max_iterations, Some(40));
        assert_eq!(
            raw.provider.as_ref().unwrap().kind,
            Some(ProviderKind::OpenAi)
        );

        let source = FileCredentialSource::new(&credentials_path);
        assert_eq!(source.resolve("openai").unwrap().expose_secret(), "sk-xxx");
    }

    #[test]
    fn run_auth_aborts_without_writing_on_eof() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let original_config = r#"
model = "keep-model"
max_iterations = 12

[provider]
kind = "anthropic"
auth_type = "api_key"
"#;
        fs::write(&config_path, original_config).unwrap();
        let credentials_path = temp.path().join("credentials");
        fs::write(&credentials_path, "anthropic = sk-keep\n").unwrap();

        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter =
            ScriptedAuthPrompter::new(vec![Some("openai".to_string()), None], vec![]);

        let err = run_auth(&paths, &mut prompter).unwrap_err();
        assert_eq!(err, AuthError::Cancelled);

        assert_eq!(fs::read_to_string(&config_path).unwrap(), original_config);
        assert_eq!(
            fs::read_to_string(&credentials_path).unwrap(),
            "anthropic = sk-keep\n"
        );
    }
}
