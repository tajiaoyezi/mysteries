//! Credential source abstractions.
//!
//! OAuth credential support is reserved for 2.0 as another
//! `CredentialSource` implementation once token storage is designed.

use secrecy::SecretString;
use std::fs;
use std::path::PathBuf;

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
    use super::{CredentialChain, CredentialSource, EnvCredentialSource, FileCredentialSource};
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
    fn env_credential_source_returns_none_when_variable_is_missing() {
        let source = EnvCredentialSource::with_lookup(|_| None);

        assert!(source.resolve("openai").is_none());
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
}
