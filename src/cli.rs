use crate::agent::message::Message;
use crate::agent::DEFAULT_SYSTEM_PROMPT;
use crate::app::{assemble_agent, load_config, select_provider, AssemblyError};
use crate::config::{write_config, Config, ConfigError, ConfigWritePatch, ProviderKind};
use crate::credential::{
    collect_credential_sources, list_credential_providers, remove_credential, write_credential,
    CredentialChain, CredentialError, CredentialOrigin, EnvCredentialSource, FileCredentialSource,
};
use crate::error::AgentError;
use crate::permission::{PermissionDecider, PermissionDecision};
use crate::provider::{DeltaSink, ToolCall};
use crate::tool::Tool;
use crate::tool::ToolContext;
use async_trait::async_trait;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CliError {
    #[error(transparent)]
    Assembly(#[from] AssemblyError),
    #[error(transparent)]
    Agent(#[from] AgentError),
    #[error("io error: {0}")]
    Io(String),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("未配置 provider。请先运行: mysteries auth login")]
    NotConfigured,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("auth cancelled")]
    Cancelled,
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
    fn select(&mut self, prompt: &str, options: &[&str]) -> Result<Option<usize>, AuthError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectAction {
    Move(usize),
    Confirm(usize),
    Cancel,
    Ignore,
}

pub fn apply_select_key(highlight: usize, len: usize, key: KeyEvent) -> SelectAction {
    match key.code {
        KeyCode::Up if len > 0 => SelectAction::Move((highlight + len - 1) % len),
        KeyCode::Down if len > 0 => SelectAction::Move((highlight + 1) % len),
        KeyCode::Enter => SelectAction::Confirm(highlight),
        KeyCode::Esc => SelectAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => SelectAction::Cancel,
        _ => SelectAction::Ignore,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderPreset {
    OpenAi,
    Anthropic,
    DeepSeek,
}

pub(crate) const OPENAI_DEFAULT_MODEL: &str = "gpt-5.5";
pub(crate) const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-4-8";
const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-v4-pro";
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

pub(crate) const WPS_CODEPLAN_OPENAI_BASE_URL: &str = "https://ai-kas.kso.net/codeplan/v1";
pub(crate) const WPS_CODEPLAN_ANTHROPIC_BASE_URL: &str =
    "https://ai-kas.kso.net/codeplan/anthropic";
pub(crate) const WPS_MODELS: &[&str] = &[
    "moonshot/kimi-k2.5",
    "deepseek/deepseek-v4-pro",
    "xiaomi/mimo-v2.5-pro",
    "ali/qwen3.7-max",
    "deepseek/deepseek-v4-flash",
    "google/gemini-3.5-flash",
    "zhipu/glm-5",
    "zhipu/glm-5.2",
];

fn login_wps_codingplan(
    prompter: &mut dyn AuthPrompter,
) -> Result<(ConfigWritePatch, String, SecretString), AuthError> {
    let protocol_options = ["OpenAI", "Anthropic"];
    let protocol_index = prompter
        .select("Select protocol", &protocol_options)?
        .ok_or(AuthError::Cancelled)?;
    let (provider_kind, base_url) = match protocol_index {
        1 => (
            ProviderKind::Anthropic,
            WPS_CODEPLAN_ANTHROPIC_BASE_URL,
        ),
        _ => (ProviderKind::OpenAi, WPS_CODEPLAN_OPENAI_BASE_URL),
    };

    let model_index = prompter
        .select("Select model", WPS_MODELS)?
        .ok_or(AuthError::Cancelled)?;
    let model = WPS_MODELS
        .get(model_index)
        .ok_or(AuthError::Cancelled)?
        .to_string();

    let key = prompter
        .read_secret("API key: ")?
        .ok_or(AuthError::Cancelled)?;

    let patch = ConfigWritePatch {
        provider_id: "wps".to_string(),
        provider_kind,
        base_url: Some(base_url.to_string()),
        model,
    };

    Ok((patch, "wps".to_string(), key))
}

fn login_wps(
    prompter: &mut dyn AuthPrompter,
) -> Result<Option<(ConfigWritePatch, String, SecretString)>, AuthError> {
    let method_options = ["OAuth2 登录(暂不支持)", "WPS CodingPlan"];
    let selected = prompter
        .select("Select WPS AI login method", &method_options)?
        .ok_or(AuthError::Cancelled)?;

    match selected {
        0 => {
            eprintln!("WPS AI OAuth2 暂不支持，后续考虑支持。");
            Ok(None)
        }
        _ => Ok(Some(login_wps_codingplan(prompter)?)),
    }
}

pub fn preset_patch(preset: ProviderPreset) -> (ConfigWritePatch, &'static str) {
    match preset {
        ProviderPreset::OpenAi => (
            ConfigWritePatch {
                provider_id: "openai".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: None,
                model: OPENAI_DEFAULT_MODEL.to_string(),
            },
            "openai",
        ),
        ProviderPreset::Anthropic => (
            ConfigWritePatch {
                provider_id: "anthropic".to_string(),
                provider_kind: ProviderKind::Anthropic,
                base_url: None,
                model: ANTHROPIC_DEFAULT_MODEL.to_string(),
            },
            "anthropic",
        ),
        ProviderPreset::DeepSeek => (
            ConfigWritePatch {
                provider_id: "deepseek".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: Some(DEEPSEEK_BASE_URL.to_string()),
                model: DEEPSEEK_DEFAULT_MODEL.to_string(),
            },
            "deepseek",
        ),
    }
}

pub fn run_auth_login_interactive(paths: &AuthPaths) -> Result<(), AuthError> {
    let mut prompter = StdinAuthPrompter;
    run_auth_login(paths, &mut prompter)
}

pub fn run_auth_logout_interactive(paths: &AuthPaths) -> Result<(), AuthError> {
    let mut prompter = StdinAuthPrompter;
    run_auth_logout(paths, &mut prompter)
}

pub fn run_auth_list(paths: &AuthPaths) -> Result<(), AuthError> {
    let entries = collect_credential_sources(&paths.credentials, |name| std::env::var(name).ok());

    if entries.is_empty() {
        eprintln!("No credentials configured. Run 'mysteries auth login' to add one.");
        return Ok(());
    }

    for entry in entries {
        let labels: Vec<&str> = entry
            .origins
            .iter()
            .map(|origin| match origin {
                CredentialOrigin::Env => "env",
                CredentialOrigin::File => "file",
            })
            .collect();
        eprintln!("{} [{}]", entry.name, labels.join(", "));
    }

    Ok(())
}

pub fn run_auth_login(paths: &AuthPaths, prompter: &mut dyn AuthPrompter) -> Result<(), AuthError> {
    let provider_options = ["OpenAI", "Anthropic", "DeepSeek", "WPS AI", "Custom"];
    let selected = prompter
        .select("Select provider", &provider_options)?
        .ok_or(AuthError::Cancelled)?;

    let outcome = match selected {
        0 => Some(login_preset(prompter, ProviderPreset::OpenAi)?),
        1 => Some(login_preset(prompter, ProviderPreset::Anthropic)?),
        2 => Some(login_preset(prompter, ProviderPreset::DeepSeek)?),
        3 => login_wps(prompter)?,
        _ => Some(login_custom(prompter)?),
    };

    let Some((patch, credential_key, key)) = outcome else {
        return Ok(());
    };

    write_config(&paths.user_config, &patch)?;
    write_credential(&paths.credentials, &credential_key, &key)?;
    eprintln!("Logged in as {}.", patch.provider_id);

    Ok(())
}

fn login_preset(
    prompter: &mut dyn AuthPrompter,
    preset: ProviderPreset,
) -> Result<(ConfigWritePatch, String, SecretString), AuthError> {
    let (patch, credential_key) = preset_patch(preset);
    let key = prompter
        .read_secret("API key: ")?
        .ok_or(AuthError::Cancelled)?;
    Ok((patch, credential_key.to_string(), key))
}

fn login_custom(
    prompter: &mut dyn AuthPrompter,
) -> Result<(ConfigWritePatch, String, SecretString), AuthError> {
    let kind_options = ["OpenAi", "Anthropic"];
    let kind_index = prompter
        .select("Select kind", &kind_options)?
        .ok_or(AuthError::Cancelled)?;
    let (provider_kind, default_id) = match kind_index {
        1 => (ProviderKind::Anthropic, "anthropic"),
        _ => (ProviderKind::OpenAi, "openai"),
    };

    let logical_name = prompter
        .read_line("Logical name (empty for kind default): ")?
        .ok_or(AuthError::Cancelled)?;
    let provider_id =
        normalize_optional_line(&logical_name).unwrap_or_else(|| default_id.to_string());

    let base_url_line = prompter
        .read_line("Base URL (empty for default): ")?
        .ok_or(AuthError::Cancelled)?;
    let base_url = normalize_optional_line(&base_url_line);

    let model_line = prompter.read_line("Model: ")?.ok_or(AuthError::Cancelled)?;
    let model = model_line.trim();
    if model.is_empty() {
        return Err(AuthError::Cancelled);
    }

    let key = prompter
        .read_secret("API key: ")?
        .ok_or(AuthError::Cancelled)?;

    let patch = ConfigWritePatch {
        provider_id: provider_id.clone(),
        provider_kind,
        base_url,
        model: model.to_string(),
    };

    Ok((patch, provider_id, key))
}

pub fn run_auth_logout(
    paths: &AuthPaths,
    prompter: &mut dyn AuthPrompter,
) -> Result<(), AuthError> {
    let providers = list_credential_providers(&paths.credentials);
    if providers.is_empty() {
        eprintln!("No configured credentials to log out.");
        return Ok(());
    }

    let options: Vec<&str> = providers.iter().map(String::as_str).collect();
    let selected = prompter.select("Select provider to log out", &options)?;
    let Some(index) = selected else {
        return Ok(());
    };

    if let Some(provider) = providers.get(index) {
        remove_credential(&paths.credentials, provider)?;
        eprintln!("Logged out of {provider}.");
    }

    Ok(())
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

    fn select(&mut self, prompt: &str, options: &[&str]) -> Result<Option<usize>, AuthError> {
        read_select(prompt, options)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SecretKeyAction {
    Append(char),
    Backspace,
    Submit,
    Cancel,
    Ignore,
}

fn apply_secret_key(key: KeyEvent) -> SecretKeyAction {
    if key.kind != KeyEventKind::Press {
        return SecretKeyAction::Ignore;
    }
    match key.code {
        KeyCode::Enter => SecretKeyAction::Submit,
        KeyCode::Esc => SecretKeyAction::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SecretKeyAction::Cancel
        }
        KeyCode::Char(_) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            SecretKeyAction::Ignore
        }
        KeyCode::Char(c) => SecretKeyAction::Append(c),
        KeyCode::Backspace | KeyCode::Delete => SecretKeyAction::Backspace,
        _ => SecretKeyAction::Ignore,
    }
}

fn read_secret_hidden(prompt: &str) -> Result<Option<SecretString>, AuthError> {
    use crossterm::event::{read, Event};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    eprint!("{prompt}");
    let _ = io::stderr().flush();

    enable_raw_mode().map_err(|_| AuthError::Cancelled)?;
    let read_result = (|| -> Result<Option<SecretString>, AuthError> {
        let mut secret = String::new();
        loop {
            let event = read().map_err(|_| AuthError::Cancelled)?;
            if let Event::Key(key) = event {
                match apply_secret_key(key) {
                    SecretKeyAction::Append(c) => {
                        secret.push(c);
                        eprint!("*");
                        let _ = io::stderr().flush();
                    }
                    SecretKeyAction::Backspace => {
                        if secret.pop().is_some() {
                            eprint!("\x08 \x08");
                            let _ = io::stderr().flush();
                        }
                    }
                    SecretKeyAction::Submit => break,
                    SecretKeyAction::Cancel => return Ok(None),
                    SecretKeyAction::Ignore => {}
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

fn read_select(prompt: &str, options: &[&str]) -> Result<Option<usize>, AuthError> {
    use crossterm::event::{read, Event, KeyEventKind};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    if options.is_empty() {
        return Ok(None);
    }

    eprintln!("{prompt}");
    let _ = io::stderr().flush();

    enable_raw_mode().map_err(|_| AuthError::Cancelled)?;
    let result = (|| -> Result<Option<usize>, AuthError> {
        let mut highlight = 0usize;
        render_select(options, highlight, true);
        loop {
            let event = read().map_err(|_| AuthError::Cancelled)?;
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match apply_select_key(highlight, options.len(), key) {
                    SelectAction::Move(idx) => {
                        highlight = idx;
                        render_select(options, highlight, false);
                    }
                    SelectAction::Confirm(idx) => return Ok(Some(idx)),
                    SelectAction::Cancel => return Ok(None),
                    SelectAction::Ignore => {}
                }
            }
        }
    })();
    let _ = disable_raw_mode();
    eprintln!();
    result
}

fn render_select(options: &[&str], highlight: usize, first: bool) {
    use crossterm::cursor::MoveToPreviousLine;
    use crossterm::execute;
    use crossterm::terminal::{Clear, ClearType};

    let mut out = io::stderr();
    if !first {
        let _ = execute!(out, MoveToPreviousLine(options.len() as u16));
    }
    for (idx, option) in options.iter().enumerate() {
        let _ = execute!(out, Clear(ClearType::CurrentLine));
        let marker = if idx == highlight { '>' } else { ' ' };
        let _ = write!(out, "\r{marker} {option}\r\n");
    }
    let _ = out.flush();
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

pub fn is_first_run(paths: &CliPaths) -> bool {
    !paths.user_config.exists() && !paths.project_config.exists()
}

pub fn load_config_or_onboard(
    paths: &CliPaths,
    prompter: &mut dyn AuthPrompter,
) -> Result<Config, CliError> {
    if is_first_run(paths) {
        run_auth_login(
            &AuthPaths {
                user_config: paths.user_config.clone(),
                credentials: paths.credentials.clone(),
            },
            prompter,
        )?;
    }
    load_config(&paths.user_config, &paths.project_config).map_err(Into::into)
}

pub async fn run_cli(paths: CliPaths, prompt: &str) -> Result<(), CliError> {
    if is_first_run(&paths) {
        return Err(CliError::NotConfigured);
    }
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
    use super::{
        apply_secret_key, apply_select_key, initial_history, parse_decision, preset_patch,
        run_auth_login, run_auth_logout, AuthError, AuthPaths, AuthPrompter, ProviderPreset,
        SecretKeyAction, SelectAction,
    };
    use crate::agent::message::Message;
    use crate::agent::DEFAULT_SYSTEM_PROMPT;
    use crate::config::{parse, write_config, ConfigError, ProviderKind};
    use crate::credential::{CredentialSource, FileCredentialSource};
    use crate::permission::PermissionDecision;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use secrecy::{ExposeSecret, SecretString};
    use std::fs;

    struct ScriptedAuthPrompter {
        lines: Vec<Option<String>>,
        secrets: Vec<Option<String>>,
        select_indices: Vec<Option<usize>>,
        line_idx: usize,
        secret_idx: usize,
        select_idx: usize,
    }

    impl ScriptedAuthPrompter {
        fn new(lines: Vec<Option<String>>, secrets: Vec<Option<String>>) -> Self {
            Self {
                lines,
                secrets,
                select_indices: Vec::new(),
                line_idx: 0,
                secret_idx: 0,
                select_idx: 0,
            }
        }

        fn with_select_script(mut self, script: Vec<Option<usize>>) -> Self {
            self.select_indices = script;
            self
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

        fn select(&mut self, _prompt: &str, _options: &[&str]) -> Result<Option<usize>, AuthError> {
            let value = self
                .select_indices
                .get(self.select_idx)
                .cloned()
                .unwrap_or(None);
            self.select_idx += 1;
            Ok(value)
        }
    }

    fn key_event(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
    fn apply_select_key_wraps_highlight_at_bounds() {
        assert_eq!(
            apply_select_key(0, 3, key_event(KeyCode::Up)),
            SelectAction::Move(2)
        );
        assert_eq!(
            apply_select_key(2, 3, key_event(KeyCode::Down)),
            SelectAction::Move(0)
        );
        assert_eq!(
            apply_select_key(1, 3, key_event(KeyCode::Down)),
            SelectAction::Move(2)
        );
        assert_eq!(
            apply_select_key(1, 3, key_event(KeyCode::Up)),
            SelectAction::Move(0)
        );
    }

    #[test]
    fn apply_select_key_confirm_and_cancel() {
        assert_eq!(
            apply_select_key(1, 3, key_event(KeyCode::Enter)),
            SelectAction::Confirm(1)
        );
        assert_eq!(
            apply_select_key(0, 2, key_event(KeyCode::Esc)),
            SelectAction::Cancel
        );
        assert_eq!(
            apply_select_key(
                0,
                2,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            ),
            SelectAction::Cancel
        );
    }

    #[test]
    fn apply_select_key_ignores_unrelated_keys() {
        assert_eq!(
            apply_select_key(0, 3, key_event(KeyCode::Char('a'))),
            SelectAction::Ignore
        );
    }

    #[test]
    fn apply_secret_key_ignores_non_press_so_leftover_release_does_not_submit_or_duplicate() {
        let release_enter =
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(apply_secret_key(release_enter), SecretKeyAction::Ignore);

        let release_char = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert_eq!(apply_secret_key(release_char), SecretKeyAction::Ignore);
    }

    #[test]
    fn apply_secret_key_maps_press_events() {
        assert_eq!(
            apply_secret_key(key_event(KeyCode::Char('a'))),
            SecretKeyAction::Append('a')
        );
        assert_eq!(
            apply_secret_key(key_event(KeyCode::Enter)),
            SecretKeyAction::Submit
        );
        assert_eq!(
            apply_secret_key(key_event(KeyCode::Esc)),
            SecretKeyAction::Cancel
        );
        assert_eq!(
            apply_secret_key(key_event(KeyCode::Backspace)),
            SecretKeyAction::Backspace
        );
        assert_eq!(
            apply_secret_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            SecretKeyAction::Cancel
        );
    }

    #[test]
    fn scripted_auth_prompter_select_returns_scripted_indices() {
        let options = ["OpenAI", "Anthropic", "DeepSeek"];
        let mut prompter =
            ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![Some(1), None]);

        assert_eq!(prompter.select("Provider", &options).unwrap(), Some(1));
        assert_eq!(prompter.select("Provider", &options).unwrap(), None);
    }

    #[test]
    fn preset_patch_maps_each_preset_to_config_and_credential_key() {
        let (openai, openai_key) = preset_patch(ProviderPreset::OpenAi);
        assert_eq!(openai.provider_id, "openai");
        assert_eq!(openai.provider_kind, ProviderKind::OpenAi);
        assert_eq!(openai.base_url, None);
        assert_eq!(openai.model, "gpt-5.5");
        assert_eq!(openai_key, "openai");

        let (anthropic, anthropic_key) = preset_patch(ProviderPreset::Anthropic);
        assert_eq!(anthropic.provider_id, "anthropic");
        assert_eq!(anthropic.provider_kind, ProviderKind::Anthropic);
        assert_eq!(anthropic.base_url, None);
        assert_eq!(anthropic.model, "claude-opus-4-8");
        assert_eq!(anthropic_key, "anthropic");

        let (deepseek, deepseek_key) = preset_patch(ProviderPreset::DeepSeek);
        assert_eq!(deepseek.provider_id, "deepseek");
        assert_eq!(deepseek.provider_kind, ProviderKind::OpenAi);
        assert_eq!(
            deepseek.base_url.as_deref(),
            Some("https://api.deepseek.com")
        );
        assert_eq!(deepseek.model, "deepseek-v4-pro");
        assert_eq!(deepseek_key, "deepseek");
    }

    #[test]
    fn run_auth_login_preset_deepseek_writes_config_and_credential() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-deepseek".to_string())])
            .with_select_script(vec![Some(2)]);

        run_auth_login(&paths, &mut prompter).unwrap();

        let raw = parse(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("deepseek-v4-pro"));
        let provider = raw.provider.as_ref().unwrap();
        assert_eq!(provider.id.as_deref(), Some("deepseek"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://api.deepseek.com")
        );

        let source = FileCredentialSource::new(&credentials_path);
        assert_eq!(
            source.resolve("deepseek").unwrap().expose_secret(),
            "sk-deepseek"
        );
    }

    #[test]
    fn run_auth_login_custom_writes_config_and_credential() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(
            vec![
                Some("myllm".to_string()),
                Some("https://my.example/v1".to_string()),
                Some("my-model".to_string()),
            ],
            vec![Some("sk-my".to_string())],
        )
        .with_select_script(vec![Some(4), Some(0)]);

        run_auth_login(&paths, &mut prompter).unwrap();

        let raw = parse(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("my-model"));
        let provider = raw.provider.as_ref().unwrap();
        assert_eq!(provider.id.as_deref(), Some("myllm"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(provider.base_url.as_deref(), Some("https://my.example/v1"));

        let source = FileCredentialSource::new(&credentials_path);
        assert_eq!(source.resolve("myllm").unwrap().expose_secret(), "sk-my");
    }

    #[test]
    fn run_auth_login_cancelled_select_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![None]);

        let result = run_auth_login(&paths, &mut prompter);

        assert_eq!(result, Err(AuthError::Cancelled));
        assert!(!config_path.exists());
        assert!(!credentials_path.exists());
    }

    #[test]
    fn run_auth_logout_removes_selected_keeps_others() {
        let temp = tempfile::tempdir().unwrap();
        let credentials_path = temp.path().join("credentials");
        fs::write(&credentials_path, "openai = sk-o\ndeepseek = sk-d\n").unwrap();
        let paths = AuthPaths {
            user_config: temp.path().join("config.toml"),
            credentials: credentials_path.clone(),
        };
        let mut prompter =
            ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![Some(1)]);

        run_auth_logout(&paths, &mut prompter).unwrap();

        let source = FileCredentialSource::new(&credentials_path);
        assert!(source.resolve("deepseek").is_none());
        assert_eq!(source.resolve("openai").unwrap().expose_secret(), "sk-o");
    }

    #[test]
    fn run_auth_logout_cancelled_select_keeps_all() {
        let temp = tempfile::tempdir().unwrap();
        let credentials_path = temp.path().join("credentials");
        fs::write(&credentials_path, "openai = sk-o\ndeepseek = sk-d\n").unwrap();
        let paths = AuthPaths {
            user_config: temp.path().join("config.toml"),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![None]);

        run_auth_logout(&paths, &mut prompter).unwrap();

        assert_eq!(
            fs::read_to_string(&credentials_path).unwrap(),
            "openai = sk-o\ndeepseek = sk-d\n"
        );
    }

    #[test]
    fn run_auth_logout_without_credentials_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: temp.path().join("config.toml"),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![]);

        run_auth_logout(&paths, &mut prompter).unwrap();

        assert!(!credentials_path.exists());
    }

    struct PanicAuthPrompter;

    impl AuthPrompter for PanicAuthPrompter {
        fn read_line(&mut self, _prompt: &str) -> Result<Option<String>, AuthError> {
            panic!("AuthPrompter must not be called");
        }

        fn read_secret(&mut self, _prompt: &str) -> Result<Option<SecretString>, AuthError> {
            panic!("AuthPrompter must not be called");
        }

        fn select(&mut self, _prompt: &str, _options: &[&str]) -> Result<Option<usize>, AuthError> {
            panic!("AuthPrompter must not be called");
        }
    }

    fn temp_cli_paths(temp: &tempfile::TempDir) -> super::CliPaths {
        super::CliPaths {
            user_config: temp.path().join("config.toml"),
            project_config: temp.path().join("mysteries.toml"),
            credentials: temp.path().join("credentials"),
            cwd: temp.path().to_path_buf(),
        }
    }

    #[test]
    fn load_config_or_onboard_first_run_openai_preset_writes_config_and_returns_ok() {
        use super::{load_config_or_onboard, OPENAI_DEFAULT_MODEL};

        let temp = tempfile::tempdir().unwrap();
        let paths = temp_cli_paths(&temp);
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-openai".to_string())])
            .with_select_script(vec![Some(0)]);

        let config = load_config_or_onboard(&paths, &mut prompter).unwrap();

        assert_eq!(config.model, OPENAI_DEFAULT_MODEL);
        assert_eq!(config.provider.kind, ProviderKind::OpenAi);
        assert!(paths.user_config.exists());
        assert!(paths.credentials.exists());

        let source = FileCredentialSource::new(&paths.credentials);
        assert_eq!(
            source.resolve("openai").unwrap().expose_secret(),
            "sk-openai"
        );
    }

    #[test]
    fn load_config_or_onboard_skips_onboarding_when_config_exists() {
        use super::{load_config_or_onboard, preset_patch, ANTHROPIC_DEFAULT_MODEL, ProviderPreset};

        let temp = tempfile::tempdir().unwrap();
        let paths = temp_cli_paths(&temp);
        let (patch, _) = preset_patch(ProviderPreset::Anthropic);
        write_config(&paths.user_config, &patch).unwrap();
        let mut prompter = PanicAuthPrompter;

        let config = load_config_or_onboard(&paths, &mut prompter).unwrap();

        assert_eq!(config.model, ANTHROPIC_DEFAULT_MODEL);
        assert_eq!(config.provider.kind, ProviderKind::Anthropic);
    }

    #[test]
    fn load_config_or_onboard_broken_config_skips_onboarding_and_returns_load_error() {
        use super::{load_config_or_onboard, CliError};
        use crate::app::AssemblyError;

        let temp = tempfile::tempdir().unwrap();
        let paths = temp_cli_paths(&temp);
        fs::write(&paths.user_config, "model = \"only-model\"\n").unwrap();
        let mut prompter = PanicAuthPrompter;

        let err = load_config_or_onboard(&paths, &mut prompter).unwrap_err();

        assert_eq!(
            err,
            CliError::Assembly(AssemblyError::Config(ConfigError::MissingField(
                "provider.kind"
            )))
        );
    }

    #[test]
    fn load_config_or_onboard_cancelled_on_first_run_writes_nothing() {
        use super::{load_config_or_onboard, CliError};

        let temp = tempfile::tempdir().unwrap();
        let paths = temp_cli_paths(&temp);
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![None]);

        let err = load_config_or_onboard(&paths, &mut prompter).unwrap_err();

        assert_eq!(err, CliError::Auth(AuthError::Cancelled));
        assert!(!paths.user_config.exists());
        assert!(!paths.credentials.exists());
    }

    #[tokio::test]
    async fn run_cli_headless_first_run_returns_not_configured() {
        use super::{run_cli, CliError};

        let temp = tempfile::tempdir().unwrap();
        let paths = temp_cli_paths(&temp);

        let err = run_cli(paths, "hi").await.unwrap_err();

        assert_eq!(err, CliError::NotConfigured);
    }

    #[test]
    fn cli_error_not_configured_display_contains_auth_login_command() {
        use super::CliError;

        assert!(
            CliError::NotConfigured
                .to_string()
                .contains("mysteries auth login")
        );
    }

    #[test]
    fn config_error_missing_field_display_is_readable() {
        assert_eq!(
            ConfigError::MissingField("model").to_string(),
            "missing required config field: model"
        );
    }

    #[test]
    fn cli_error_assembly_transparent_passthrough_missing_field() {
        use super::CliError;
        use crate::app::AssemblyError;

        assert_eq!(
            CliError::Assembly(AssemblyError::Config(ConfigError::MissingField("model")))
                .to_string(),
            "missing required config field: model"
        );
    }

    #[test]
    fn login_wps_codingplan_openai_first_model_returns_expected_patch() {
        use super::{
            login_wps_codingplan, WPS_CODEPLAN_OPENAI_BASE_URL, WPS_MODELS,
        };

        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-wps".to_string())])
            .with_select_script(vec![Some(0), Some(0)]);

        let (patch, credential_key, key) = login_wps_codingplan(&mut prompter).unwrap();

        assert_eq!(patch.provider_id, "wps");
        assert_eq!(patch.provider_kind, ProviderKind::OpenAi);
        assert_eq!(
            patch.base_url.as_deref(),
            Some(WPS_CODEPLAN_OPENAI_BASE_URL)
        );
        assert_eq!(patch.model, WPS_MODELS[0]);
        assert_eq!(credential_key, "wps");
        assert_eq!(key.expose_secret(), "sk-wps");
    }

    #[test]
    fn login_wps_codingplan_anthropic_protocol_uses_anthropic_endpoint() {
        use super::{
            login_wps_codingplan, WPS_CODEPLAN_ANTHROPIC_BASE_URL, WPS_MODELS,
        };

        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-wps2".to_string())])
            .with_select_script(vec![Some(1), Some(0)]);

        let (patch, credential_key, key) = login_wps_codingplan(&mut prompter).unwrap();

        assert_eq!(patch.provider_id, "wps");
        assert_eq!(patch.provider_kind, ProviderKind::Anthropic);
        assert_eq!(
            patch.base_url.as_deref(),
            Some(WPS_CODEPLAN_ANTHROPIC_BASE_URL)
        );
        assert_eq!(patch.model, WPS_MODELS[0]);
        assert_eq!(credential_key, "wps");
        assert_eq!(key.expose_secret(), "sk-wps2");
    }

    #[test]
    fn login_wps_codingplan_selects_nth_model_from_catalog() {
        use super::{login_wps_codingplan, WPS_MODELS};

        let model_index = 3usize;
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-wps".to_string())])
            .with_select_script(vec![Some(0), Some(model_index)]);

        let (patch, _, _) = login_wps_codingplan(&mut prompter).unwrap();

        assert_eq!(patch.model, WPS_MODELS[model_index]);
    }

    #[test]
    fn login_wps_codingplan_cancelled_at_protocol_returns_cancelled() {
        use super::login_wps_codingplan;

        let mut prompter =
            ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![None]);

        assert_eq!(
            login_wps_codingplan(&mut prompter).unwrap_err(),
            AuthError::Cancelled
        );
    }

    #[test]
    fn login_wps_codingplan_cancelled_at_model_returns_cancelled() {
        use super::login_wps_codingplan;

        let mut prompter =
            ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![Some(0), None]);

        assert_eq!(
            login_wps_codingplan(&mut prompter).unwrap_err(),
            AuthError::Cancelled
        );
    }

    #[test]
    fn login_wps_codingplan_cancelled_at_key_returns_cancelled() {
        use super::login_wps_codingplan;

        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![None])
            .with_select_script(vec![Some(0), Some(0)]);

        assert_eq!(
            login_wps_codingplan(&mut prompter).unwrap_err(),
            AuthError::Cancelled
        );
    }

    #[test]
    fn run_auth_login_wps_oauth2_placeholder_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter =
            ScriptedAuthPrompter::new(vec![], vec![]).with_select_script(vec![Some(3), Some(0)]);

        run_auth_login(&paths, &mut prompter).unwrap();

        assert!(!config_path.exists());
        assert!(!credentials_path.exists());
    }

    #[test]
    fn run_auth_login_wps_codingplan_writes_config_and_credential() {
        use super::{WPS_CODEPLAN_OPENAI_BASE_URL, WPS_MODELS};

        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let credentials_path = temp.path().join("credentials");
        let paths = AuthPaths {
            user_config: config_path.clone(),
            credentials: credentials_path.clone(),
        };
        let mut prompter = ScriptedAuthPrompter::new(vec![], vec![Some("sk-wps".to_string())])
            .with_select_script(vec![Some(3), Some(1), Some(0), Some(0)]);

        run_auth_login(&paths, &mut prompter).unwrap();

        let raw = parse(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some(WPS_MODELS[0]));
        let provider = raw.provider.as_ref().unwrap();
        assert_eq!(provider.id.as_deref(), Some("wps"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            provider.base_url.as_deref(),
            Some(WPS_CODEPLAN_OPENAI_BASE_URL)
        );

        let source = FileCredentialSource::new(&credentials_path);
        assert_eq!(source.resolve("wps").unwrap().expose_secret(), "sk-wps");
    }

    #[test]
    fn run_auth_login_provider_menu_lists_wps_ai_before_custom() {
        struct CaptureSelectPrompter {
            captured: Option<Vec<String>>,
        }

        impl AuthPrompter for CaptureSelectPrompter {
            fn read_line(&mut self, _prompt: &str) -> Result<Option<String>, AuthError> {
                Ok(None)
            }

            fn read_secret(&mut self, _prompt: &str) -> Result<Option<SecretString>, AuthError> {
                Ok(None)
            }

            fn select(
                &mut self,
                _prompt: &str,
                options: &[&str],
            ) -> Result<Option<usize>, AuthError> {
                self.captured = Some(options.iter().map(|option| (*option).to_string()).collect());
                Ok(None)
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let paths = AuthPaths {
            user_config: temp.path().join("config.toml"),
            credentials: temp.path().join("credentials"),
        };
        let mut prompter = CaptureSelectPrompter { captured: None };

        assert_eq!(
            run_auth_login(&paths, &mut prompter),
            Err(AuthError::Cancelled)
        );
        assert_eq!(
            prompter.captured.as_deref(),
            Some(vec![
                "OpenAI".to_string(),
                "Anthropic".to_string(),
                "DeepSeek".to_string(),
                "WPS AI".to_string(),
                "Custom".to_string(),
            ])
            .as_deref()
        );
    }
}
