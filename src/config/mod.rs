use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use thiserror::Error;

pub const DEFAULT_MAX_ITERATIONS: u32 = 50;
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_COMPACT_TRIGGER_RATIO: f32 = 0.8;
pub const DEFAULT_KEEP_RECENT_TURNS: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub provider: ProviderConfig,
    pub model: String,
    pub max_iterations: u32,
    pub timeout_secs: u64,
    pub model_context_window: Option<u32>,
    pub compact_trigger_ratio: f32,
    pub keep_recent_turns: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct RawConfig {
    #[serde(default)]
    pub provider: Option<RawProviderConfig>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub model_context_window: Option<u32>,
    #[serde(default)]
    pub compact_trigger_ratio: Option<f32>,
    #[serde(default)]
    pub keep_recent_turns: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub auth_type: AuthType,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RawProviderConfig {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub kind: Option<ProviderKind>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_type: Option<AuthType>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Mock,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    ApiKey,
    // OAuth 留给 2.0 作为另一个凭据来源形态。
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("failed to parse TOML config: {0}")]
    Toml(String),
    #[error("missing required config field: {0}")]
    MissingField(&'static str),
    #[error("invalid config value: {0}")]
    InvalidValue(&'static str),
    #[error("failed to write config: {0}")]
    Write(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigWritePatch {
    pub provider_id: String,
    pub provider_kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
}

pub fn read_raw_config(path: &Path) -> Result<RawConfig, ConfigError> {
    match fs::read_to_string(path) {
        Ok(source) => parse(&source),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(RawConfig::default()),
        Err(err) => Err(ConfigError::Write(err.to_string())),
    }
}

pub fn write_config(path: &Path, patch: &ConfigWritePatch) -> Result<(), ConfigError> {
    let mut raw = read_raw_config(path)?;

    raw.model = Some(patch.model.clone());
    let mut provider = raw.provider.take().unwrap_or_default();
    provider.kind = Some(patch.provider_kind.clone());
    provider.id = Some(patch.provider_id.clone());
    provider.base_url = patch.base_url.clone();
    if provider.auth_type.is_none() {
        provider.auth_type = Some(AuthType::ApiKey);
    }
    raw.provider = Some(provider);

    let serialized =
        toml::to_string_pretty(&raw).map_err(|err| ConfigError::Toml(err.to_string()))?;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| ConfigError::Write(err.to_string()))?;
        }
    }
    fs::write(path, serialized.as_bytes()).map_err(|err| ConfigError::Write(err.to_string()))?;
    Ok(())
}

pub fn parse(source: &str) -> Result<RawConfig, ConfigError> {
    toml::from_str(source).map_err(|err| ConfigError::Toml(err.to_string()))
}

pub fn merge(user: RawConfig, project: RawConfig) -> RawConfig {
    RawConfig {
        provider: merge_provider(user.provider, project.provider),
        model: project.model.or(user.model),
        max_iterations: project.max_iterations.or(user.max_iterations),
        timeout_secs: project.timeout_secs.or(user.timeout_secs),
        model_context_window: project.model_context_window.or(user.model_context_window),
        compact_trigger_ratio: project.compact_trigger_ratio.or(user.compact_trigger_ratio),
        keep_recent_turns: project.keep_recent_turns.or(user.keep_recent_turns),
    }
}

fn merge_provider(
    user: Option<RawProviderConfig>,
    project: Option<RawProviderConfig>,
) -> Option<RawProviderConfig> {
    match (user, project) {
        (None, None) => None,
        (Some(user), None) => Some(user),
        (None, Some(project)) => Some(project),
        (Some(user), Some(project)) => Some(RawProviderConfig {
            id: project.id.or(user.id),
            kind: project.kind.or(user.kind),
            base_url: project.base_url.or(user.base_url),
            auth_type: project.auth_type.or(user.auth_type),
        }),
    }
}

pub fn resolve(raw: RawConfig) -> Result<Config, ConfigError> {
    let model = raw.model.ok_or(ConfigError::MissingField("model"))?;
    let provider = raw.provider.unwrap_or_default();
    let kind = provider
        .kind
        .ok_or(ConfigError::MissingField("provider.kind"))?;
    let provider = ProviderConfig {
        id: provider
            .id
            .unwrap_or_else(|| default_provider_id_for_kind(&kind).to_string()),
        kind,
        base_url: provider.base_url,
        auth_type: provider.auth_type.unwrap_or(AuthType::ApiKey),
    };

    let compact_trigger_ratio = raw
        .compact_trigger_ratio
        .unwrap_or(DEFAULT_COMPACT_TRIGGER_RATIO);
    if compact_trigger_ratio <= 0.0 || compact_trigger_ratio > 1.0 {
        return Err(ConfigError::InvalidValue(
            "compact_trigger_ratio must be in (0, 1]",
        ));
    }

    Ok(Config {
        provider,
        model,
        max_iterations: raw.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS),
        timeout_secs: raw.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
        model_context_window: raw.model_context_window,
        compact_trigger_ratio,
        keep_recent_turns: raw.keep_recent_turns.unwrap_or(DEFAULT_KEEP_RECENT_TURNS),
    })
}

fn default_provider_id_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "openai",
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::Mock => "mock",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        merge, parse, read_raw_config, resolve, write_config, AuthType, ConfigError,
        ConfigWritePatch, ProviderKind, DEFAULT_COMPACT_TRIGGER_RATIO, DEFAULT_KEEP_RECENT_TURNS,
        DEFAULT_TIMEOUT_SECS,
    };
    use std::fs;

    #[test]
    fn parse_partial_toml_sets_some_and_missing_fields_to_none() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
kind = "openai"
"#,
        )
        .unwrap();

        assert_eq!(raw.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(raw.max_iterations, None);
        assert_eq!(raw.timeout_secs, None);

        let provider = raw.provider.unwrap();
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(provider.id, None);
        assert_eq!(provider.base_url, None);
        assert_eq!(provider.auth_type, None);
    }

    #[test]
    fn parse_invalid_toml_returns_config_error() {
        let err = parse("model = [").unwrap_err();

        assert!(matches!(err, ConfigError::Toml(_)));
    }

    #[test]
    fn merge_project_overrides_scalars_and_inherits_missing_user_values() {
        let user = parse(
            r#"
model = "user-model"
timeout_secs = 30
"#,
        )
        .unwrap();
        let project = parse(
            r#"
model = "project-model"
"#,
        )
        .unwrap();

        let merged = merge(user, project);

        assert_eq!(merged.model.as_deref(), Some("project-model"));
        assert_eq!(merged.timeout_secs, Some(30));
        assert_eq!(merged.max_iterations, None);
    }

    #[test]
    fn merge_provider_nested_fields_recursively() {
        let user = parse(
            r#"
[provider]
kind = "openai"
base_url = "https://user.example/v1"
auth_type = "api_key"
"#,
        )
        .unwrap();
        let project = parse(
            r#"
[provider]
base_url = "https://project.example/v1"
"#,
        )
        .unwrap();

        let merged = merge(user, project);
        let provider = merged.provider.unwrap();

        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://project.example/v1")
        );
        assert!(provider.auth_type.is_some());
    }

    #[test]
    fn merge_leaves_fields_none_when_both_layers_omit_them() {
        let merged = merge(parse("").unwrap(), parse("").unwrap());

        assert_eq!(merged.model, None);
        assert_eq!(merged.timeout_secs, None);
        assert_eq!(merged.provider, None);
    }

    #[test]
    fn resolve_complete_raw_config_applies_defaults() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"
timeout_secs = 45

[provider]
kind = "openai"
auth_type = "api_key"
"#,
        )
        .unwrap();

        let config = resolve(raw).unwrap();

        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.provider.kind, ProviderKind::OpenAi);
        assert_eq!(config.provider.auth_type, AuthType::ApiKey);
        assert_eq!(config.max_iterations, 50);
        assert_eq!(config.timeout_secs, 45);
    }

    #[test]
    fn resolve_defaults_timeout_when_missing() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
kind = "mock"
auth_type = "api_key"
"#,
        )
        .unwrap();

        let config = resolve(raw).unwrap();

        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn resolve_returns_missing_field_for_required_model() {
        let raw = parse(
            r#"
[provider]
kind = "openai"
auth_type = "api_key"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(err, ConfigError::MissingField("model"));
    }

    #[test]
    fn resolve_returns_missing_field_for_required_provider_kind() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
auth_type = "api_key"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(err, ConfigError::MissingField("provider.kind"));
    }

    fn minimal_raw() -> String {
        r#"
model = "gpt-4o-mini"

[provider]
kind = "mock"
auth_type = "api_key"
"#
        .to_string()
    }

    #[test]
    fn resolve_applies_compaction_defaults_when_unset() {
        let config = resolve(parse(&minimal_raw()).unwrap()).unwrap();

        assert_eq!(config.compact_trigger_ratio, DEFAULT_COMPACT_TRIGGER_RATIO);
        assert_eq!(config.keep_recent_turns, DEFAULT_KEEP_RECENT_TURNS);
        assert_eq!(config.model_context_window, None);
    }

    #[test]
    fn merge_overrides_compaction_fields_from_project_layer() {
        let user = parse(
            r#"
model = "user-model"
model_context_window = 128000
compact_trigger_ratio = 0.9

[provider]
kind = "mock"
auth_type = "api_key"
"#,
        )
        .unwrap();
        let project = parse(
            r#"
model = "project-model"
compact_trigger_ratio = 0.7
"#,
        )
        .unwrap();

        let merged = merge(user, project);
        let config = resolve(merged).unwrap();

        assert_eq!(config.model, "project-model");
        assert_eq!(config.model_context_window, Some(128000));
        assert_eq!(config.compact_trigger_ratio, 0.7);
        assert_eq!(config.keep_recent_turns, DEFAULT_KEEP_RECENT_TURNS);
    }

    #[test]
    fn resolve_rejects_compact_trigger_ratio_out_of_range() {
        for invalid in ["0.0", "-0.1", "1.5", "2.0"] {
            let source = format!(
                r#"
model = "gpt-4o-mini"
compact_trigger_ratio = {invalid}

[provider]
kind = "mock"
auth_type = "api_key"
"#
            );
            let err = resolve(parse(&source).unwrap()).unwrap_err();
            assert!(
                matches!(err, ConfigError::InvalidValue(_)),
                "ratio {invalid} should be invalid: {err:?}"
            );
        }
    }

    #[test]
    fn write_config_merges_model_and_preserves_other_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
model = "old"
max_iterations = 40

[provider]
kind = "anthropic"
auth_type = "api_key"
"#,
        )
        .unwrap();

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: String::new(),
                provider_kind: ProviderKind::Anthropic,
                base_url: None,
                model: "new".to_string(),
            },
        )
        .unwrap();

        let raw = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("new"));
        assert_eq!(raw.max_iterations, Some(40));
        assert_eq!(
            raw.provider.as_ref().unwrap().kind,
            Some(ProviderKind::Anthropic)
        );
    }

    #[test]
    fn write_config_merges_provider_kind_and_base_url() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
model = "old"
max_iterations = 40

[provider]
kind = "anthropic"
base_url = "https://old.example/v1"
auth_type = "api_key"
"#,
        )
        .unwrap();

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: String::new(),
                provider_kind: ProviderKind::OpenAi,
                base_url: Some("https://new.example/v1".to_string()),
                model: "new".to_string(),
            },
        )
        .unwrap();

        let raw = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("new"));
        assert_eq!(raw.max_iterations, Some(40));
        let provider = raw.provider.unwrap();
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(provider.base_url.as_deref(), Some("https://new.example/v1"));
    }

    #[test]
    fn write_config_creates_file_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: String::new(),
                provider_kind: ProviderKind::OpenAi,
                base_url: None,
                model: "m".to_string(),
            },
        )
        .unwrap();

        let raw = read_raw_config(&path).unwrap();
        assert_eq!(raw.model.as_deref(), Some("m"));
        assert_eq!(
            raw.provider.as_ref().unwrap().kind,
            Some(ProviderKind::OpenAi)
        );
    }

    #[test]
    fn parse_old_config_without_provider_id() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
kind = "openai"
auth_type = "api_key"
"#,
        )
        .unwrap();

        assert_eq!(raw.provider.as_ref().unwrap().id, None);
    }

    #[test]
    fn parse_provider_id_when_present() {
        let raw = parse(
            r#"
model = "deepseek-v4-pro"

[provider]
id = "deepseek"
kind = "openai"
"#,
        )
        .unwrap();

        let provider = raw.provider.unwrap();
        assert_eq!(provider.id.as_deref(), Some("deepseek"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
    }

    #[test]
    fn resolve_provider_id_falls_back_to_kind_default_name() {
        for (kind, expected_id) in [
            ("openai", "openai"),
            ("anthropic", "anthropic"),
            ("mock", "mock"),
        ] {
            let raw = parse(&format!(
                r#"
model = "test-model"

[provider]
kind = "{kind}"
auth_type = "api_key"
"#
            ))
            .unwrap();

            let config = resolve(raw).unwrap();
            assert_eq!(config.provider.id, expected_id);
        }
    }

    #[test]
    fn resolve_provider_id_uses_explicit_id_when_set() {
        let raw = parse(
            r#"
model = "deepseek-v4-pro"

[provider]
id = "deepseek"
kind = "openai"
auth_type = "api_key"
"#,
        )
        .unwrap();

        let config = resolve(raw).unwrap();

        assert_eq!(config.provider.id, "deepseek");
        assert_eq!(config.provider.kind, ProviderKind::OpenAi);
    }

    #[test]
    fn merge_provider_id_field_level() {
        let user = parse(
            r#"
[provider]
id = "openai"
kind = "openai"
"#,
        )
        .unwrap();
        let project = parse(
            r#"
[provider]
id = "deepseek"
"#,
        )
        .unwrap();

        let merged = merge(user, project);
        let provider = merged.provider.unwrap();

        assert_eq!(provider.id.as_deref(), Some("deepseek"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
    }

    #[test]
    fn write_config_persists_provider_id() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
model = "old"
max_iterations = 40

[provider]
kind = "anthropic"
auth_type = "api_key"
"#,
        )
        .unwrap();

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: "deepseek".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: Some("https://api.deepseek.com".to_string()),
                model: "deepseek-v4-pro".to_string(),
            },
        )
        .unwrap();

        let raw = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(raw.max_iterations, Some(40));
        let provider = raw.provider.unwrap();
        assert_eq!(provider.id.as_deref(), Some("deepseek"));
        assert_eq!(provider.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://api.deepseek.com")
        );
    }
}
