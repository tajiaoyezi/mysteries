use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_MAX_ITERATIONS: u32 = 8;
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub provider: ProviderConfig,
    pub model: String,
    pub max_iterations: u32,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RawConfig {
    #[serde(default)]
    pub provider: Option<RawProviderConfig>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub auth_type: AuthType,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RawProviderConfig {
    #[serde(default)]
    pub kind: Option<ProviderKind>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_type: Option<AuthType>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Mock,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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
            kind: project.kind.or(user.kind),
            base_url: project.base_url.or(user.base_url),
            auth_type: project.auth_type.or(user.auth_type),
        }),
    }
}

pub fn resolve(raw: RawConfig) -> Result<Config, ConfigError> {
    let model = raw.model.ok_or(ConfigError::MissingField("model"))?;
    let provider = raw.provider.unwrap_or_default();
    let provider = ProviderConfig {
        kind: provider
            .kind
            .ok_or(ConfigError::MissingField("provider.kind"))?,
        base_url: provider.base_url,
        auth_type: provider.auth_type.unwrap_or(AuthType::ApiKey),
    };

    Ok(Config {
        provider,
        model,
        max_iterations: raw.max_iterations.unwrap_or(DEFAULT_MAX_ITERATIONS),
        timeout_secs: raw.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        merge, parse, resolve, AuthType, ConfigError, ProviderKind, DEFAULT_MAX_ITERATIONS,
        DEFAULT_TIMEOUT_SECS,
    };

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
        assert_eq!(config.max_iterations, DEFAULT_MAX_ITERATIONS);
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
}
