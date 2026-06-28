//! Credential source abstractions.
//!
//! OAuth credential support is reserved for 2.0 as another
//! `CredentialSource` implementation once token storage is designed.

use secrecy::{ExposeSecret, SecretString};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use thiserror::Error;

// struct OAuthCredentialSource; // 2.0 落地

pub trait CredentialSource: Send + Sync {
    fn resolve(&self, provider: &str) -> Option<SecretString>;
}

type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct EnvCredentialSource {
    lookup: EnvLookup,
}

impl EnvCredentialSource {
    pub fn new() -> Self {
        Self::with_lookup(|name| std::env::var(name).ok())
    }

    pub fn with_lookup<L>(lookup: L) -> Self
    where
        L: Fn(&str) -> Option<String> + Send + Sync + 'static,
    {
        let lookup: EnvLookup = Box::new(lookup);
        Self { lookup }
    }
}

impl Default for EnvCredentialSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialSource for EnvCredentialSource {
    fn resolve(&self, provider: &str) -> Option<SecretString> {
        let env_name = match provider {
            "openai" => "OPENAI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            _ => return None,
        };

        (self.lookup)(env_name).map(SecretString::from)
    }
}

pub struct FileCredentialSource {
    path: PathBuf,
}

impl FileCredentialSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl CredentialSource for FileCredentialSource {
    fn resolve(&self, provider: &str) -> Option<SecretString> {
        let content = fs::read_to_string(&self.path).ok()?;

        for line in content.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((line_provider, key)) = line.split_once('=') else {
                continue;
            };
            if line_provider.trim() == provider {
                return Some(SecretString::from(key.trim().to_string()));
            }
        }

        None
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error("failed to write credentials: {0}")]
    Write(String),
}

pub fn write_credential(
    path: &Path,
    provider: &str,
    key: &SecretString,
) -> Result<(), CredentialError> {
    let plain_key = key.expose_secret();

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(CredentialError::Write(format!(
                "read {}: {}",
                path.display(),
                err.kind()
            )));
        }
    };

    let updated = upsert_credential_line(&content, provider, plain_key);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent) {
                return Err(CredentialError::Write(format!(
                    "create dir {}: {}",
                    parent.display(),
                    err.kind()
                )));
            }
        }
    }

    if let Err(err) = fs::write(path, updated.as_bytes()) {
        return Err(CredentialError::Write(format!(
            "write {}: {}",
            path.display(),
            err.kind()
        )));
    }

    #[cfg(unix)]
    restrict_permissions(path)?;

    Ok(())
}

fn upsert_credential_line(content: &str, provider: &str, key: &str) -> String {
    let mut replaced = false;
    let mut out_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some((line_provider, _)) = trimmed.split_once('=') {
                if line_provider.trim() == provider {
                    out_lines.push(format!("{provider} = {key}"));
                    replaced = true;
                    continue;
                }
            }
        }
        out_lines.push(line.to_string());
    }

    if !replaced {
        out_lines.push(format!("{provider} = {key}"));
    }

    let mut result = out_lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), CredentialError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|err| CredentialError::Write(format!("chmod {}: {}", path.display(), err.kind())))
}

pub struct CredentialChain(Vec<Box<dyn CredentialSource>>);

impl CredentialChain {
    pub fn new(sources: Vec<Box<dyn CredentialSource>>) -> Self {
        Self(sources)
    }

    pub fn resolve(&self, provider: &str) -> Option<SecretString> {
        for source in &self.0 {
            if let Some(secret) = source.resolve(provider) {
                return Some(secret);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        write_credential, CredentialChain, CredentialSource, EnvCredentialSource,
        FileCredentialSource,
    };
    use secrecy::{ExposeSecret, SecretString};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeCredentialSource {
        value: Option<&'static str>,
    }

    impl CredentialSource for FakeCredentialSource {
        fn resolve(&self, provider: &str) -> Option<SecretString> {
            if provider == "openai" {
                self.value.map(SecretString::from)
            } else {
                None
            }
        }
    }

    #[test]
    fn credential_source_trait_object_returns_secret_when_provider_matches() {
        let source: Box<dyn CredentialSource> = Box::new(FakeCredentialSource {
            value: Some("sk-test"),
        });

        let secret = source.resolve("openai").unwrap();

        assert_eq!(secret.expose_secret(), "sk-test");
    }

    #[test]
    fn credential_source_trait_object_returns_none_when_provider_is_missing() {
        let source: Box<dyn CredentialSource> = Box::new(FakeCredentialSource { value: None });

        assert!(source.resolve("openai").is_none());
        assert!(source.resolve("anthropic").is_none());
    }

    #[test]
    fn env_credential_source_resolves_openai_from_injected_lookup() {
        let source = EnvCredentialSource::with_lookup(|name| {
            if name == "OPENAI_API_KEY" {
                Some("sk-env".to_string())
            } else {
                None
            }
        });

        let secret = source.resolve("openai").unwrap();

        assert_eq!(secret.expose_secret(), "sk-env");
    }

    #[test]
    fn env_credential_source_resolves_anthropic_from_injected_lookup() {
        let source = EnvCredentialSource::with_lookup(|name| {
            if name == "ANTHROPIC_API_KEY" {
                Some("sk-ant-env".to_string())
            } else {
                None
            }
        });

        let secret = source.resolve("anthropic").unwrap();

        assert_eq!(secret.expose_secret(), "sk-ant-env");
    }

    #[test]
    fn env_credential_source_returns_none_when_variable_is_missing() {
        let source = EnvCredentialSource::with_lookup(|_| None);

        assert!(source.resolve("openai").is_none());
        assert!(source.resolve("anthropic").is_none());
    }

    #[test]
    fn file_credential_source_resolves_matching_provider_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credentials");
        fs::write(
            &path,
            "\n# ignored\nanthropic = sk-anthropic\n openai = sk-file \n",
        )
        .unwrap();
        let source = FileCredentialSource::new(&path);

        let secret = source.resolve("openai").unwrap();

        assert_eq!(secret.expose_secret(), "sk-file");
    }

    #[test]
    fn file_credential_source_returns_none_for_missing_provider_or_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credentials");
        fs::write(&path, "anthropic = sk-anthropic\n").unwrap();

        assert!(FileCredentialSource::new(&path).resolve("openai").is_none());
        assert!(FileCredentialSource::new(temp.path().join("missing"))
            .resolve("openai")
            .is_none());
    }

    struct CountingCredentialSource {
        value: Option<&'static str>,
        calls: Arc<AtomicUsize>,
    }

    impl CredentialSource for CountingCredentialSource {
        fn resolve(&self, _provider: &str) -> Option<SecretString> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.value.map(SecretString::from)
        }
    }

    fn counting_source(
        value: Option<&'static str>,
    ) -> (Box<dyn CredentialSource>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Box::new(CountingCredentialSource {
                value,
                calls: calls.clone(),
            }),
            calls,
        )
    }

    #[test]
    fn credential_chain_returns_env_value_first_and_short_circuits_file() {
        let (env, env_calls) = counting_source(Some("sk-env"));
        let (file, file_calls) = counting_source(Some("sk-file"));
        let chain = CredentialChain::new(vec![env, file]);

        let secret = chain.resolve("openai").unwrap();

        assert_eq!(secret.expose_secret(), "sk-env");
        assert_eq!(env_calls.load(Ordering::SeqCst), 1);
        assert_eq!(file_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn credential_chain_falls_back_to_file_when_env_is_missing() {
        let (env, env_calls) = counting_source(None);
        let (file, file_calls) = counting_source(Some("sk-file"));
        let chain = CredentialChain::new(vec![env, file]);

        let secret = chain.resolve("openai").unwrap();

        assert_eq!(secret.expose_secret(), "sk-file");
        assert_eq!(env_calls.load(Ordering::SeqCst), 1);
        assert_eq!(file_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn credential_chain_returns_none_when_all_sources_are_missing() {
        let (env, _) = counting_source(None);
        let (file, _) = counting_source(None);
        let chain = CredentialChain::new(vec![env, file]);

        assert!(chain.resolve("openai").is_none());
    }

    #[test]
    fn secret_string_debug_output_does_not_expose_plaintext() {
        let secret = SecretString::from("sk-secret-xxx");

        let debug = format!("{secret:?}");

        assert!(!debug.contains("sk-secret-xxx"));
        assert!(debug.contains("REDACTED"));
        assert_eq!(secret.expose_secret(), "sk-secret-xxx");
    }

    #[test]
    fn write_credential_upserts_new_and_replaces_existing_preserving_other_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credentials");
        fs::write(
            &path,
            "# header comment\nanthropic = sk-a\n# trailing comment\n",
        )
        .unwrap();

        write_credential(&path, "openai", &SecretString::from("sk-o".to_string())).unwrap();
        write_credential(&path, "anthropic", &SecretString::from("sk-a2".to_string())).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("openai = sk-o"));
        assert!(content.contains("anthropic = sk-a2"));
        assert!(!content.contains("anthropic = sk-a\n"));
        assert!(content.contains("# header comment"));
        assert!(content.contains("# trailing comment"));

        let source = FileCredentialSource::new(&path);
        assert_eq!(source.resolve("openai").unwrap().expose_secret(), "sk-o");
        assert_eq!(
            source.resolve("anthropic").unwrap().expose_secret(),
            "sk-a2"
        );
    }

    #[test]
    fn write_credential_error_does_not_leak_plaintext() {
        let temp = tempfile::tempdir().unwrap();
        let blocker = temp.path().join("blocker");
        fs::write(&blocker, "not a directory").unwrap();
        let path = blocker.join("credentials");

        let secret = SecretString::from("sk-super-secret-key".to_string());
        let err = write_credential(&path, "openai", &secret).unwrap_err();

        let message = err.to_string();
        assert!(!message.contains("sk-super-secret-key"));
    }

    #[cfg(unix)]
    #[test]
    fn write_credential_sets_owner_only_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("credentials");

        write_credential(&path, "openai", &SecretString::from("sk-o".to_string())).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
