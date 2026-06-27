use crate::agent::Agent;
use crate::config::{self, Config, ConfigError, ProviderKind, RawConfig};
use crate::credential::CredentialChain;
use crate::permission::PermissionDecider;
use crate::provider::mock::MockProvider;
use crate::provider::openai::OpenAiProvider;
use crate::provider::{FinishReason, ModelResponse, Provider};
use crate::tool::edit::{EditFileTool, WriteFileTool};
use crate::tool::fs::{GlobTool, GrepTool, ListDirTool, ReadFileTool};
use crate::tool::shell::RunShellTool;
use crate::tool::ToolRegistry;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AssemblyError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("failed to read config {path}: {message}")]
    Io { path: String, message: String },
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
}

pub fn load_config(
    user_path: impl AsRef<Path>,
    project_path: impl AsRef<Path>,
) -> Result<Config, AssemblyError> {
    let user = read_config_layer(user_path.as_ref())?;
    let project = read_config_layer(project_path.as_ref())?;

    Ok(config::resolve(config::merge(user, project))?)
}

fn read_config_layer(path: &Path) -> Result<RawConfig, AssemblyError> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(config::parse(&source)?),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(RawConfig::default()),
        Err(err) => Err(AssemblyError::Io {
            path: path.display().to_string(),
            message: err.to_string(),
        }),
    }
}

pub fn select_provider(
    config: &Config,
    credentials: CredentialChain,
) -> Result<Box<dyn Provider>, AssemblyError> {
    match config.provider.kind {
        ProviderKind::OpenAi => {
            let provider = match &config.provider.base_url {
                Some(base_url) => OpenAiProvider::new(base_url, credentials),
                None => OpenAiProvider::default(credentials),
            };
            Ok(Box::new(provider))
        }
        ProviderKind::Anthropic => Err(AssemblyError::UnsupportedProvider("anthropic".to_string())),
        ProviderKind::Mock => Ok(Box::new(MockProvider::new(vec![ModelResponse {
            text: "mock response".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
        }]))),
    }
}

pub fn default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ListDirTool)).unwrap();
    registry.register(Box::new(ReadFileTool)).unwrap();
    registry.register(Box::new(GlobTool)).unwrap();
    registry.register(Box::new(GrepTool)).unwrap();
    registry.register(Box::new(WriteFileTool)).unwrap();
    registry.register(Box::new(EditFileTool)).unwrap();
    registry.register(Box::new(RunShellTool)).unwrap();
    registry
}

pub fn assemble_agent(
    provider: Box<dyn Provider>,
    config: &Config,
    decider: Box<dyn PermissionDecider>,
) -> Agent {
    Agent::new(
        provider,
        default_registry(),
        decider,
        config.model.clone(),
        config.max_iterations,
    )
}

#[cfg(test)]
mod tests {
    use super::{assemble_agent, default_registry, load_config, select_provider, AssemblyError};
    use crate::agent::message::Message;
    use crate::config::{AuthType, Config, ConfigError, ProviderConfig, ProviderKind};
    use crate::credential::CredentialChain;
    use crate::permission::{PermissionDecider, PermissionDecision};
    use crate::provider::mock::MockProvider;
    use crate::provider::{DeltaSink, ModelRequest};
    use crate::provider::{FinishReason, ModelResponse, ToolCall};
    use crate::tool::{Tool, ToolContext};
    use async_trait::async_trait;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn load_config_merges_user_and_project_with_project_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let user_path = temp.path().join("user.toml");
        let project_path = temp.path().join("project.toml");
        fs::write(
            &user_path,
            r#"
model = "user-model"
max_iterations = 3
timeout_secs = 45

[provider]
kind = "openai"
base_url = "https://user.example/v1"
auth_type = "api_key"
"#,
        )
        .unwrap();
        fs::write(
            &project_path,
            r#"
model = "project-model"

[provider]
base_url = "https://project.example/v1"
"#,
        )
        .unwrap();

        let config = load_config(&user_path, &project_path).unwrap();

        assert_eq!(config.model, "project-model");
        assert_eq!(config.max_iterations, 3);
        assert_eq!(config.timeout_secs, 45);
        assert_eq!(config.provider.kind, ProviderKind::OpenAi);
        assert_eq!(
            config.provider.base_url.as_deref(),
            Some("https://project.example/v1")
        );
        assert_eq!(config.provider.auth_type, AuthType::ApiKey);
    }

    #[test]
    fn load_config_tolerates_missing_user_file_and_resolves_project_only() {
        let temp = tempfile::tempdir().unwrap();
        let user_path = temp.path().join("missing-user.toml");
        let project_path = temp.path().join("project.toml");
        fs::write(
            &project_path,
            r#"
model = "project-model"

[provider]
kind = "mock"
"#,
        )
        .unwrap();

        let config = load_config(&user_path, &project_path).unwrap();

        assert_eq!(config.model, "project-model");
        assert_eq!(config.provider.kind, ProviderKind::Mock);
    }

    #[test]
    fn load_config_returns_error_for_invalid_toml() {
        let temp = tempfile::tempdir().unwrap();
        let user_path = temp.path().join("user.toml");
        let project_path = temp.path().join("project.toml");
        fs::write(&user_path, "model = [").unwrap();

        let err = load_config(&user_path, &project_path).unwrap_err();

        assert!(matches!(err, AssemblyError::Config(ConfigError::Toml(_))));
    }

    #[test]
    fn load_config_returns_error_when_required_fields_are_missing() {
        let temp = tempfile::tempdir().unwrap();
        let user_path = temp.path().join("user.toml");
        let project_path = temp.path().join("project.toml");
        fs::write(
            &project_path,
            r#"
[provider]
kind = "mock"
"#,
        )
        .unwrap();

        let err = load_config(&user_path, &project_path).unwrap_err();

        assert_eq!(
            err,
            AssemblyError::Config(ConfigError::MissingField("model"))
        );
    }

    struct NoopSink;

    impl DeltaSink for NoopSink {
        fn on_text(&self, _text: &str) {}
    }

    fn config_for(kind: ProviderKind) -> Config {
        Config {
            provider: ProviderConfig {
                kind,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "test-model".to_string(),
            max_iterations: 4,
            timeout_secs: 30,
        }
    }

    fn empty_credentials() -> CredentialChain {
        CredentialChain::new(Vec::new())
    }

    #[test]
    fn select_provider_returns_openai_provider_without_network() {
        let provider = select_provider(&config_for(ProviderKind::OpenAi), empty_credentials())
            .expect("OpenAi should be selectable offline");

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn select_provider_rejects_unsupported_anthropic() {
        let err = match select_provider(&config_for(ProviderKind::Anthropic), empty_credentials()) {
            Ok(provider) => panic!(
                "expected unsupported provider error, got {}",
                provider.name()
            ),
            Err(err) => err,
        };

        assert_eq!(
            err,
            AssemblyError::UnsupportedProvider("anthropic".to_string())
        );
    }

    #[tokio::test]
    async fn select_provider_returns_mock_provider_that_can_complete_offline() {
        let provider = select_provider(&config_for(ProviderKind::Mock), empty_credentials())
            .expect("Mock should be selectable offline");
        let sink = NoopSink;

        let response = provider
            .complete(
                ModelRequest {
                    model: "test-model".to_string(),
                    messages: Vec::new(),
                    tools: Vec::new(),
                    max_tokens: None,
                },
                &sink,
            )
            .await
            .unwrap();

        assert_eq!(provider.name(), "mock");
        assert!(!response.text.is_empty());
    }

    struct AllowAll;

    #[async_trait]
    impl PermissionDecider for AllowAll {
        async fn decide(&self, _call: &ToolCall, _tool: &dyn Tool) -> PermissionDecision {
            PermissionDecision::Allow
        }
    }

    fn ctx(root: PathBuf) -> ToolContext {
        ToolContext {
            cwd: root,
            max_output_bytes: 4096,
        }
    }

    #[test]
    fn default_registry_contains_all_builtin_tools() {
        let registry = default_registry();
        let mut names = registry
            .schemas()
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();
        names.sort();

        assert_eq!(
            names,
            vec![
                "edit_file",
                "glob",
                "grep",
                "list_dir",
                "read_file",
                "run_shell",
                "write_file",
            ]
        );
    }

    #[tokio::test]
    async fn assemble_agent_uses_config_model_and_dispatches_default_tools() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            provider: ProviderConfig {
                kind: ProviderKind::Mock,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "configured-model".to_string(),
            max_iterations: 2,
            timeout_secs: 30,
        };
        let provider = Arc::new(MockProvider::new(vec![
            ModelResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "write_file".to_string(),
                    arguments: json!({ "path": "note.txt", "content": "created" }),
                }],
                finish_reason: FinishReason::ToolCalls,
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
            },
        ]));
        let agent = assemble_agent(Box::new(provider.clone()), &config, Box::new(AllowAll));
        let sink = NoopSink;
        let mut history = vec![Message::User("write note".to_string())];

        let text = agent
            .run(&mut history, &ctx(temp.path().to_path_buf()), &sink)
            .await
            .unwrap();

        assert_eq!(text, "done");
        assert_eq!(
            fs::read_to_string(temp.path().join("note.txt")).unwrap(),
            "created"
        );
        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].model, "configured-model");
        assert_eq!(recorded[0].tools.len(), 7);
    }
}
