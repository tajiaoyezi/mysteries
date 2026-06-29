use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    pub active: Option<String>,
    #[serde(default)]
    pub providers: Option<BTreeMap<String, RawProviderProfile>>,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RawProviderProfile {
    #[serde(default)]
    pub kind: Option<ProviderKind>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
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
    migrate_legacy_into_providers(&mut raw);

    let mut providers = raw.providers.take().unwrap_or_default();
    providers.insert(
        patch.provider_id.clone(),
        RawProviderProfile {
            kind: Some(patch.provider_kind.clone()),
            base_url: patch.base_url.clone(),
            model: Some(patch.model.clone()),
            auth_type: Some(AuthType::ApiKey),
        },
    );
    raw.providers = Some(providers);
    raw.active = Some(patch.provider_id.clone());
    raw.provider = None;
    raw.model = None;

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

fn migrate_legacy_into_providers(raw: &mut RawConfig) {
    let legacy_provider = match raw.provider.take() {
        Some(provider) => provider,
        None => return,
    };
    let legacy_model = raw.model.take();
    let mut providers = raw.providers.take().unwrap_or_default();
    let legacy_id = legacy_provider.id.clone().unwrap_or_else(|| {
        legacy_provider
            .kind
            .as_ref()
            .map(|kind| default_provider_id_for_kind(kind).to_string())
            .unwrap_or_else(|| "openai".to_string())
    });

    if providers.contains_key(&legacy_id) {
        raw.provider = Some(legacy_provider);
        raw.model = legacy_model;
        raw.providers = Some(providers);
        return;
    }

    providers.insert(
        legacy_id,
        RawProviderProfile {
            kind: legacy_provider.kind,
            base_url: legacy_provider.base_url,
            model: legacy_model,
            auth_type: legacy_provider.auth_type.or(Some(AuthType::ApiKey)),
        },
    );
    raw.providers = Some(providers);
}

pub fn parse(source: &str) -> Result<RawConfig, ConfigError> {
    toml::from_str(source).map_err(|err| ConfigError::Toml(err.to_string()))
}

pub fn merge(user: RawConfig, project: RawConfig) -> RawConfig {
    RawConfig {
        provider: merge_provider(user.provider, project.provider),
        model: project.model.or(user.model),
        active: project.active.or(user.active),
        providers: merge_providers(user.providers, project.providers),
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

fn merge_providers(
    user: Option<BTreeMap<String, RawProviderProfile>>,
    project: Option<BTreeMap<String, RawProviderProfile>>,
) -> Option<BTreeMap<String, RawProviderProfile>> {
    match (user, project) {
        (None, None) => None,
        (Some(user), None) => Some(user),
        (None, Some(project)) => Some(project),
        (Some(mut user), Some(project)) => {
            for (id, project_profile) in project {
                user.entry(id)
                    .and_modify(|existing| {
                        *existing =
                            merge_provider_profile(existing.clone(), project_profile.clone());
                    })
                    .or_insert(project_profile);
            }
            Some(user)
        }
    }
}

fn merge_provider_profile(
    user: RawProviderProfile,
    project: RawProviderProfile,
) -> RawProviderProfile {
    RawProviderProfile {
        kind: project.kind.or(user.kind),
        base_url: project.base_url.or(user.base_url),
        model: project.model.or(user.model),
        auth_type: project.auth_type.or(user.auth_type),
    }
}

pub fn resolve(raw: RawConfig) -> Result<Config, ConfigError> {
    let (provider, model) = if raw
        .providers
        .as_ref()
        .is_some_and(|providers| !providers.is_empty())
    {
        resolve_multi_provider(raw.active, raw.providers.unwrap())?
    } else {
        resolve_legacy_provider(raw.model, raw.provider)?
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

fn resolve_multi_provider(
    active: Option<String>,
    providers: BTreeMap<String, RawProviderProfile>,
) -> Result<(ProviderConfig, String), ConfigError> {
    let (provider_id, profile) = match active {
        Some(active_id) => {
            let profile = providers.get(&active_id).ok_or(ConfigError::InvalidValue(
                "active references unknown provider",
            ))?;
            (active_id, profile.clone())
        }
        None if providers.len() == 1 => {
            let (provider_id, profile) =
                providers.iter().next().expect("providers map is non-empty");
            (provider_id.clone(), profile.clone())
        }
        None => return Err(ConfigError::MissingField("active")),
    };

    let kind = profile
        .kind
        .ok_or(ConfigError::MissingField("provider.kind"))?;
    let model = profile.model.ok_or(ConfigError::MissingField("model"))?;
    let provider = ProviderConfig {
        id: provider_id,
        kind,
        base_url: profile.base_url,
        auth_type: profile.auth_type.unwrap_or(AuthType::ApiKey),
    };

    Ok((provider, model))
}

fn resolve_legacy_provider(
    model: Option<String>,
    provider: Option<RawProviderConfig>,
) -> Result<(ProviderConfig, String), ConfigError> {
    let model = model.ok_or(ConfigError::MissingField("model"))?;
    let provider = provider.unwrap_or_default();
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

    Ok((provider, model))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderProfile {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
    pub auth_type: AuthType,
}

pub fn resolve_provider_profiles(raw: &RawConfig) -> Vec<ProviderProfile> {
    if raw
        .providers
        .as_ref()
        .is_some_and(|providers| !providers.is_empty())
    {
        let mut profiles = Vec::new();
        for (id, profile) in raw.providers.as_ref().unwrap() {
            let Some(kind) = profile.kind.clone() else {
                continue;
            };
            let Some(model) = profile.model.as_ref().filter(|model| !model.is_empty()) else {
                continue;
            };
            profiles.push(ProviderProfile {
                id: id.clone(),
                kind,
                base_url: profile.base_url.clone(),
                model: model.clone(),
                auth_type: profile.auth_type.clone().unwrap_or(AuthType::ApiKey),
            });
        }
        return profiles;
    }

    let Some(provider) = raw.provider.as_ref() else {
        return Vec::new();
    };
    let Some(kind) = provider.kind.clone() else {
        return Vec::new();
    };
    let Some(model) = raw.model.as_ref().filter(|model| !model.is_empty()) else {
        return Vec::new();
    };

    vec![ProviderProfile {
        id: provider
            .id
            .clone()
            .unwrap_or_else(|| default_provider_id_for_kind(&kind).to_string()),
        kind,
        base_url: provider.base_url.clone(),
        model: model.clone(),
        auth_type: provider.auth_type.clone().unwrap_or(AuthType::ApiKey),
    }]
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
        assert_eq!(raw.active.as_deref(), Some(""));
        assert_eq!(raw.max_iterations, Some(40));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        let profile = raw.providers.as_ref().unwrap().get("").unwrap();
        assert_eq!(profile.kind, Some(ProviderKind::Anthropic));
        assert_eq!(profile.model.as_deref(), Some("new"));
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
        assert_eq!(raw.active.as_deref(), Some(""));
        assert_eq!(raw.max_iterations, Some(40));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        let profile = raw.providers.as_ref().unwrap().get("").unwrap();
        assert_eq!(profile.kind, Some(ProviderKind::OpenAi));
        assert_eq!(profile.base_url.as_deref(), Some("https://new.example/v1"));
        assert_eq!(profile.model.as_deref(), Some("new"));
    }

    #[test]
    fn write_config_creates_file_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: "openai".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: None,
                model: "m".to_string(),
            },
        )
        .unwrap();

        let raw = read_raw_config(&path).unwrap();
        assert_eq!(raw.active.as_deref(), Some("openai"));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        let profile = raw.providers.as_ref().unwrap().get("openai").unwrap();
        assert_eq!(profile.kind, Some(ProviderKind::OpenAi));
        assert_eq!(profile.model.as_deref(), Some("m"));
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
        assert_eq!(raw.active.as_deref(), Some("deepseek"));
        assert_eq!(raw.max_iterations, Some(40));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        let profile = raw.providers.as_ref().unwrap().get("deepseek").unwrap();
        assert_eq!(profile.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            profile.base_url.as_deref(),
            Some("https://api.deepseek.com")
        );
        assert_eq!(profile.model.as_deref(), Some("deepseek-v4-pro"));
    }

    #[test]
    fn resolve_multi_provider_active_hit_selects_profile() {
        let raw = parse(
            r#"
active = "wps"

[providers.wps]
kind = "openai"
base_url = "https://ai-kas.kso.net/codeplan/v1"
model = "m-wps"

[providers.deepseek]
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
"#,
        )
        .unwrap();

        let config = resolve(raw).unwrap();

        assert_eq!(config.provider.id, "wps");
        assert_eq!(config.provider.kind, ProviderKind::OpenAi);
        assert_eq!(
            config.provider.base_url.as_deref(),
            Some("https://ai-kas.kso.net/codeplan/v1")
        );
        assert_eq!(config.model, "m-wps");
    }

    #[test]
    fn resolve_multi_provider_single_entry_without_active() {
        let raw = parse(
            r#"
[providers.deepseek]
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
"#,
        )
        .unwrap();

        let config = resolve(raw).unwrap();

        assert_eq!(config.provider.id, "deepseek");
        assert_eq!(config.model, "deepseek-v4-pro");
    }

    #[test]
    fn resolve_multi_provider_multiple_without_active_requires_active_field() {
        let raw = parse(
            r#"
[providers.wps]
kind = "openai"
model = "m-wps"

[providers.deepseek]
kind = "openai"
model = "deepseek-v4-pro"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(err, ConfigError::MissingField("active"));
    }

    #[test]
    fn resolve_multi_provider_unknown_active_is_invalid_value() {
        let raw = parse(
            r#"
active = "nope"

[providers.wps]
kind = "openai"
model = "m-wps"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(
            err,
            ConfigError::InvalidValue("active references unknown provider")
        );
    }

    #[test]
    fn resolve_without_providers_map_falls_back_to_legacy_single_provider() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
id = "deepseek"
kind = "openai"
auth_type = "api_key"
"#,
        )
        .unwrap();

        assert!(raw.providers.is_none());

        let config = resolve(raw).unwrap();

        assert_eq!(config.provider.id, "deepseek");
        assert_eq!(config.model, "gpt-4o-mini");
    }

    #[test]
    fn resolve_multi_provider_selected_profile_missing_kind() {
        let raw = parse(
            r#"
active = "wps"

[providers.wps]
model = "m-wps"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(err, ConfigError::MissingField("provider.kind"));
    }

    #[test]
    fn resolve_multi_provider_selected_profile_missing_model() {
        let raw = parse(
            r#"
model = "legacy"
active = "wps"

[providers.wps]
kind = "openai"
"#,
        )
        .unwrap();

        let err = resolve(raw).unwrap_err();

        assert_eq!(err, ConfigError::MissingField("model"));
    }

    #[test]
    fn parse_new_multi_provider_schema() {
        let raw = parse(
            r#"
active = "wps"

[providers.wps]
kind = "openai"
base_url = "https://ai-kas.kso.net/codeplan/v1"
model = "m-wps"

[providers.deepseek]
kind = "openai"
model = "deepseek-v4-pro"
"#,
        )
        .unwrap();

        assert_eq!(raw.active.as_deref(), Some("wps"));
        let providers = raw.providers.unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers.get("wps").unwrap().model.as_deref(),
            Some("m-wps")
        );
        assert_eq!(
            providers.get("deepseek").unwrap().model.as_deref(),
            Some("deepseek-v4-pro")
        );
    }

    #[test]
    fn parse_legacy_schema_leaves_active_and_providers_none() {
        let raw = parse(
            r#"
model = "gpt-4o-mini"

[provider]
kind = "openai"
"#,
        )
        .unwrap();

        assert_eq!(raw.active, None);
        assert_eq!(raw.providers, None);
    }

    #[test]
    fn merge_providers_union_with_field_level_override() {
        let user = parse(
            r#"
[providers.a]
kind = "openai"
model = "a-model"

[providers.b]
kind = "openai"
model = "u-b"
"#,
        )
        .unwrap();
        let project = parse(
            r#"
[providers.b]
model = "p-b"

[providers.c]
kind = "anthropic"
model = "c-model"
"#,
        )
        .unwrap();

        let merged = merge(user, project);
        let providers = merged.providers.unwrap();

        assert_eq!(providers.len(), 3);
        assert_eq!(
            providers.get("a").unwrap().model.as_deref(),
            Some("a-model")
        );
        assert_eq!(providers.get("b").unwrap().model.as_deref(), Some("p-b"));
        assert_eq!(
            providers.get("c").unwrap().model.as_deref(),
            Some("c-model")
        );
    }

    #[test]
    fn merge_active_project_overrides_user() {
        let user = parse(
            r#"
active = "a"
"#,
        )
        .unwrap();
        let project = parse(
            r#"
active = "c"
"#,
        )
        .unwrap();

        let merged = merge(user, project);

        assert_eq!(merged.active.as_deref(), Some("c"));
    }

    #[test]
    fn write_config_upsert_preserves_other_providers_and_sets_active() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
active = "deepseek"
max_iterations = 40

[providers.deepseek]
kind = "openai"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-pro"
"#,
        )
        .unwrap();

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: "wps".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: Some("https://ai-kas.kso.net/codeplan/v1".to_string()),
                model: "m-wps".to_string(),
            },
        )
        .unwrap();

        let raw = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.active.as_deref(), Some("wps"));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        assert_eq!(raw.max_iterations, Some(40));
        let providers = raw.providers.as_ref().unwrap();
        assert_eq!(
            providers.get("deepseek").unwrap().model.as_deref(),
            Some("deepseek-v4-pro")
        );
        let wps = providers.get("wps").unwrap();
        assert_eq!(wps.kind, Some(ProviderKind::OpenAi));
        assert_eq!(
            wps.base_url.as_deref(),
            Some("https://ai-kas.kso.net/codeplan/v1")
        );
        assert_eq!(wps.model.as_deref(), Some("m-wps"));
    }

    #[test]
    fn write_config_migrates_legacy_provider_before_upsert() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
model = "m-ds"
max_iterations = 40

[provider]
id = "deepseek"
kind = "openai"
base_url = "https://api.deepseek.com"
"#,
        )
        .unwrap();

        write_config(
            &path,
            &ConfigWritePatch {
                provider_id: "wps".to_string(),
                provider_kind: ProviderKind::OpenAi,
                base_url: Some("https://ai-kas.kso.net/codeplan/v1".to_string()),
                model: "m-wps".to_string(),
            },
        )
        .unwrap();

        let raw = parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw.active.as_deref(), Some("wps"));
        assert_eq!(raw.provider, None);
        assert_eq!(raw.model, None);
        let providers = raw.providers.as_ref().unwrap();
        let deepseek = providers.get("deepseek").unwrap();
        assert_eq!(deepseek.model.as_deref(), Some("m-ds"));
        assert_eq!(
            deepseek.base_url.as_deref(),
            Some("https://api.deepseek.com")
        );
        assert_eq!(
            providers.get("wps").unwrap().model.as_deref(),
            Some("m-wps")
        );
    }

    #[test]
    fn resolve_provider_profiles_returns_all_new_schema_entries() {
        use super::resolve_provider_profiles;

        let raw = parse(
            r#"
active = "anthropic"

[providers.anthropic]
kind = "anthropic"
model = "claude-opus-4-8"

[providers.wps]
kind = "openai"
base_url = "https://ai-kas.kso.net/codeplan/v1"
model = "zhipu/glm-5.2"
"#,
        )
        .unwrap();

        let profiles = resolve_provider_profiles(&raw);
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].id, "anthropic");
        assert_eq!(profiles[0].kind, ProviderKind::Anthropic);
        assert_eq!(profiles[0].model, "claude-opus-4-8");
        assert_eq!(profiles[1].id, "wps");
        assert_eq!(profiles[1].kind, ProviderKind::OpenAi);
        assert_eq!(profiles[1].model, "zhipu/glm-5.2");
    }

    #[test]
    fn resolve_provider_profiles_falls_back_to_legacy_single_provider() {
        use super::resolve_provider_profiles;

        let raw = parse(
            r#"
model = "gpt-4o"

[provider]
kind = "openai"
"#,
        )
        .unwrap();

        let profiles = resolve_provider_profiles(&raw);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "openai");
        assert_eq!(profiles[0].kind, ProviderKind::OpenAi);
        assert_eq!(profiles[0].model, "gpt-4o");
    }

    #[test]
    fn resolve_provider_profiles_returns_empty_without_any_provider() {
        use super::resolve_provider_profiles;

        let raw = parse("max_iterations = 8").unwrap();
        assert!(resolve_provider_profiles(&raw).is_empty());
    }

    #[test]
    fn resolve_provider_profiles_skips_incomplete_entries() {
        use super::resolve_provider_profiles;

        let raw = parse(
            r#"
[providers.good]
kind = "anthropic"
model = "claude-opus-4-8"

[providers.missing-kind]
model = "orphan-model"

[providers.missing-model]
kind = "openai"
"#,
        )
        .unwrap();

        let profiles = resolve_provider_profiles(&raw);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "good");
    }
}
