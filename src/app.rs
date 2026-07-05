use crate::agent::context::ContextStrategy;
use crate::agent::{Agent, Compacting, CompactionSettings};
use crate::config::{self, Config, ConfigError, ProviderKind, RawConfig};
use crate::credential::CredentialChain;
use crate::permission::PermissionDecider;
use crate::provider::anthropic::AnthropicProvider;
use crate::provider::mock::MockProvider;
use crate::provider::openai::OpenAiProvider;
use crate::provider::{FinishReason, ModelResponse, Provider};
use crate::tool::ask::AskUserTool;
use crate::tool::edit::{EditFileTool, WriteFileTool};
use crate::tool::fs::{GlobTool, GrepTool, ListDirTool, ReadFileTool};
use crate::tool::plan::{SubmitPlanTool, UpdatePlanTool};
use crate::tool::shell::RunShellTool;
use crate::tool::web::{ReqwestFetcher, WebFetchTool, WebSearchTool};
use crate::tool::ToolRegistry;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
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
    Ok(config::resolve(load_merged_raw(user_path, project_path)?)?)
}

pub fn load_merged_raw(
    user_path: impl AsRef<Path>,
    project_path: impl AsRef<Path>,
) -> Result<RawConfig, AssemblyError> {
    let user = read_config_layer(user_path.as_ref())?;
    let project = read_config_layer(project_path.as_ref())?;
    Ok(config::merge(user, project))
}

pub fn provider_profiles_from_paths(
    user_path: impl AsRef<Path>,
    project_path: impl AsRef<Path>,
) -> Result<std::collections::BTreeMap<String, config::ProviderProfile>, AssemblyError> {
    let raw = load_merged_raw(user_path, project_path)?;
    Ok(config::resolve_provider_profiles(&raw)
        .into_iter()
        .map(|profile| (profile.id.clone(), profile))
        .collect())
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
) -> Result<Arc<dyn Provider>, AssemblyError> {
    let attempt_timeout = Duration::from_secs(config.timeout_secs);
    let credential_name = config.provider.id.as_str();
    let provider: Box<dyn Provider> = match config.provider.kind {
        ProviderKind::OpenAi => {
            let provider = match &config.provider.base_url {
                Some(base_url) => OpenAiProvider::with_attempt_timeout(
                    base_url,
                    credentials,
                    credential_name,
                    attempt_timeout,
                ),
                None => OpenAiProvider::default_with_attempt_timeout(
                    credentials,
                    credential_name,
                    attempt_timeout,
                ),
            };
            Box::new(provider)
        }
        ProviderKind::Anthropic => {
            let provider = match &config.provider.base_url {
                Some(base_url) => AnthropicProvider::with_attempt_timeout(
                    base_url,
                    credentials,
                    credential_name,
                    attempt_timeout,
                ),
                None => AnthropicProvider::default_with_attempt_timeout(
                    credentials,
                    credential_name,
                    attempt_timeout,
                ),
            };
            Box::new(provider)
        }
        ProviderKind::Mock => Box::new(MockProvider::new(vec![ModelResponse {
            text: "mock response".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }])),
    };
    Ok(Arc::from(provider))
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
        .register(Box::new(WebFetchTool::new(Box::new(ReqwestFetcher::new()))))
        .unwrap();
    registry
        .register(Box::new(WebSearchTool::new(
            Box::new(ReqwestFetcher::new()),
        )))
        .unwrap();
    registry
}

fn compaction_settings(config: &Config) -> CompactionSettings {
    CompactionSettings {
        model_context_window: config.model_context_window,
        compact_trigger_ratio: config.compact_trigger_ratio,
        keep_recent_turns: config.keep_recent_turns,
    }
}

/// 压缩默认启用:`model_context_window` 未配时,有效窗口由 `Compacting` 在
/// 判定时按当前 model 解析(内置表 / 保守默认,见 `provider::model_meta`)。
pub fn build_compacting(provider: Arc<dyn Provider>, config: &Config) -> Compacting {
    Compacting::new(provider, config.model.clone(), compaction_settings(config))
}

pub struct AssembledAgent {
    pub agent: Agent,
    pub compacting: Compacting,
}

pub fn assemble_agent(
    provider: Arc<dyn Provider>,
    config: &Config,
    decider: Box<dyn PermissionDecider>,
    plan_approver: Option<Box<dyn crate::tool::plan::PlanApprover>>,
    user_prompter: Option<Box<dyn crate::tool::ask::UserPrompter>>,
    progress_reporter: Option<Box<dyn crate::tool::plan::PlanProgressReporter>>,
) -> AssembledAgent {
    let compacting = build_compacting(provider.clone(), config);
    let strategy: Box<dyn ContextStrategy> = Box::new(build_compacting(provider.clone(), config));
    let mut registry = default_registry();
    if let Some(approver) = plan_approver {
        registry
            .register(Box::new(SubmitPlanTool::new(approver)))
            .expect("submit_plan should register once");
    }
    if let Some(prompter) = user_prompter {
        registry
            .register(Box::new(AskUserTool::new(prompter)))
            .expect("ask_user should register once");
    }
    if let Some(reporter) = progress_reporter {
        registry
            .register(Box::new(UpdatePlanTool::new(reporter)))
            .expect("update_plan should register once");
    }
    let mut agent = Agent::new(
        provider,
        registry,
        decider,
        config.model.clone(),
        config.max_iterations,
    );
    agent.set_strategy(strategy);
    AssembledAgent { agent, compacting }
}

#[cfg(test)]
mod tests {
    use super::{assemble_agent, default_registry, load_config, select_provider, AssemblyError};
    use crate::agent::message::Message;
    use crate::config::{
        AuthType, Config, ConfigError, ProviderConfig, ProviderKind, DEFAULT_COMPACT_TRIGGER_RATIO,
        DEFAULT_KEEP_RECENT_TURNS,
    };
    use crate::credential::{CredentialChain, FileCredentialSource};
    use crate::error::ProviderError;
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
    use std::time::Duration;

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
                id: String::new(),
                kind,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "test-model".to_string(),
            allowed_commands: Vec::new(),
            max_iterations: 4,
            timeout_secs: 30,
            model_context_window: None,
            compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
        }
    }

    fn config_for_with_timeout(kind: ProviderKind, timeout_secs: u64) -> Config {
        Config {
            timeout_secs,
            ..config_for(kind)
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
    fn select_provider_returns_anthropic_provider_without_network() {
        let provider = select_provider(&config_for(ProviderKind::Anthropic), empty_credentials())
            .expect("Anthropic should be selectable offline");

        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn select_provider_injects_configured_attempt_timeout_without_network() {
        for kind in [ProviderKind::OpenAi, ProviderKind::Anthropic] {
            let provider = select_provider(&config_for_with_timeout(kind, 12), empty_credentials())
                .expect("provider should be selectable offline");

            assert_eq!(provider.attempt_timeout(), Some(Duration::from_secs(12)));
        }
    }

    #[tokio::test]
    async fn select_provider_injects_provider_id_as_credential_name() {
        let temp = tempfile::tempdir().unwrap();
        let cred_path = temp.path().join("credentials");
        fs::write(&cred_path, "openai = sk-openai-only\n").unwrap();

        let mut config = config_for(ProviderKind::OpenAi);
        config.provider.id = "deepseek".to_string();
        config.provider.base_url = Some("http://127.0.0.1:9/v1".to_string());
        let chain = CredentialChain::new(vec![Box::new(FileCredentialSource::new(&cred_path))]);

        let provider = select_provider(&config, chain).unwrap();
        let sink = NoopSink;
        let err = provider
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
            .unwrap_err();

        assert_eq!(err, ProviderError::Auth);
    }

    #[tokio::test]
    async fn select_provider_uses_resolved_id_so_matching_key_passes_auth() {
        let temp = tempfile::tempdir().unwrap();
        let cred_path = temp.path().join("credentials");
        fs::write(&cred_path, "deepseek = sk-deepseek\n").unwrap();

        let mut config = config_for(ProviderKind::OpenAi);
        config.provider.id = "deepseek".to_string();
        config.provider.base_url = Some("http://127.0.0.1:9/v1".to_string());
        let chain = CredentialChain::new(vec![Box::new(FileCredentialSource::new(&cred_path))]);

        let provider = select_provider(&config, chain).unwrap();
        let sink = NoopSink;
        let err = provider
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
            .unwrap_err();

        assert_ne!(err, ProviderError::Auth);
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
                "web_fetch",
                "web_search",
                "write_file",
            ]
        );
    }

    #[tokio::test]
    async fn assemble_agent_uses_config_model_and_dispatches_default_tools() {
        let temp = tempfile::tempdir().unwrap();
        let config = Config {
            provider: ProviderConfig {
                id: String::new(),
                kind: ProviderKind::Mock,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "configured-model".to_string(),
            allowed_commands: Vec::new(),
            max_iterations: 2,
            timeout_secs: 30,
            model_context_window: None,
            compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
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
                usage: None,
            },
            ModelResponse {
                text: "done".to_string(),
                tool_calls: Vec::new(),
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ]));
        let assembled = assemble_agent(
            provider.clone(),
            &config,
            Box::new(AllowAll),
            None,
            None,
            None,
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("write note".to_string())];

        let text = assembled
            .agent
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
        assert_eq!(recorded[0].tools.len(), 9);
    }

    #[tokio::test]
    async fn assemble_agent_compacts_by_default_and_respects_explicit_window() {
        use crate::agent::context::ContextStrategy;
        use crate::provider::Usage;

        fn config_with_window(window: Option<u32>) -> Config {
            Config {
                provider: ProviderConfig {
                    id: String::new(),
                    kind: ProviderKind::Mock,
                    base_url: None,
                    auth_type: AuthType::ApiKey,
                },
                model: "configured-model".to_string(),
                allowed_commands: Vec::new(),
                max_iterations: 4,
                timeout_secs: 30,
                model_context_window: window,
                compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
                keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
            }
        }
        let history = vec![
            Message::System("sys".to_string()),
            Message::User("one".to_string()),
            Message::Assistant {
                text: "r1".to_string(),
                tool_calls: Vec::new(),
            },
            Message::User("two".to_string()),
            Message::Assistant {
                text: "r2".to_string(),
                tool_calls: Vec::new(),
            },
        ];
        let usage = Usage {
            input_tokens: 60_000,
            output_tokens: 10,
        };

        // 未配 window:未知 model 走保守默认 65_536,60k > 52_428(0.8 阈值)触发压缩。
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "SUMMARY".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }]));
        let assembled = assemble_agent(
            provider,
            &config_with_window(None),
            Box::new(AllowAll),
            None,
            None,
            None,
        );
        let compacted = assembled
            .compacting
            .prepare(&history, Some(&usage))
            .await
            .expect("prepare should succeed");
        assert_ne!(
            compacted, history,
            "without configured window compaction must still trigger via default window"
        );

        // 显式 128k:同一 usage 不触发(60k < 102_400),原样返回。
        let provider = Arc::new(MockProvider::new(Vec::new()));
        let assembled = assemble_agent(
            provider,
            &config_with_window(Some(128_000)),
            Box::new(AllowAll),
            None,
            None,
            None,
        );
        let unchanged = assembled
            .compacting
            .prepare(&history, Some(&usage))
            .await
            .expect("prepare should succeed");
        assert_eq!(
            unchanged, history,
            "explicit 128k window must not trigger at 60k input tokens"
        );
    }

    #[test]
    fn assemble_agent_some_registers_update_plan_with_twelve_schemas() {
        use crate::tool::ask::{Answer, AskUserTool, MockPrompter};
        use crate::tool::plan::{
            MockPlanApprover, MockPlanProgressReporter, PlanDecision, SubmitPlanTool,
            UpdatePlanTool,
        };

        let mut registry = default_registry();
        registry
            .register(Box::new(SubmitPlanTool::new(Box::new(
                MockPlanApprover::new(PlanDecision::Approve),
            ))))
            .unwrap();
        registry
            .register(Box::new(AskUserTool::new(Box::new(MockPrompter::new(
                Answer {
                    selected: Vec::new(),
                    supplement: None,
                },
            )))))
            .unwrap();
        registry
            .register(Box::new(UpdatePlanTool::new(Box::new(
                MockPlanProgressReporter::new(),
            ))))
            .unwrap();

        let schemas = registry.schemas();
        assert_eq!(schemas.len(), 12);
        assert!(schemas.iter().any(|schema| schema.name == "update_plan"));
    }

    #[tokio::test]
    async fn assemble_agent_some_drives_eleven_tools_in_normal_mode() {
        use crate::tool::ask::{Answer, MockPrompter};
        use crate::tool::plan::{MockPlanApprover, MockPlanProgressReporter, PlanDecision};

        let config = Config {
            provider: ProviderConfig {
                id: String::new(),
                kind: ProviderKind::Mock,
                base_url: None,
                auth_type: AuthType::ApiKey,
            },
            model: "configured-model".to_string(),
            allowed_commands: Vec::new(),
            max_iterations: 2,
            timeout_secs: 30,
            model_context_window: None,
            compact_trigger_ratio: DEFAULT_COMPACT_TRIGGER_RATIO,
            keep_recent_turns: DEFAULT_KEEP_RECENT_TURNS,
        };
        let provider = Arc::new(MockProvider::new(vec![ModelResponse {
            text: "ok".to_string(),
            tool_calls: Vec::new(),
            finish_reason: FinishReason::Stop,
            usage: None,
        }]));
        let assembled = assemble_agent(
            provider.clone(),
            &config,
            Box::new(AllowAll),
            Some(Box::new(MockPlanApprover::new(PlanDecision::Approve))),
            Some(Box::new(MockPrompter::new(Answer {
                selected: Vec::new(),
                supplement: None,
            }))),
            Some(Box::new(MockPlanProgressReporter::new())),
        );
        let sink = NoopSink;
        let mut history = vec![Message::User("hello".to_string())];
        let _ = assembled
            .agent
            .run(
                &mut history,
                &ToolContext {
                    cwd: PathBuf::from("."),
                    max_output_bytes: 4096,
                },
                &sink,
            )
            .await;

        let recorded = provider.recorded_requests();
        assert_eq!(recorded[0].tools.len(), 11);
        assert!(recorded[0]
            .tools
            .iter()
            .any(|tool| tool.name == "update_plan"));
    }
}
