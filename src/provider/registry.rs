#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub provider_id: &'static str,
    pub models: &'static [&'static str],
}

const WPS_MODELS: &[&str] = &[
    "zhipu/glm-5.2",
    "zhipu/glm-5",
    "moonshot/kimi-k2.5",
    "deepseek/deepseek-v4-pro",
    "deepseek/deepseek-v4-flash",
    "ali/qwen3.7-max",
    "xiaomi/mimo-v2.5-pro",
    "google/gemini-3.5-flash",
];

const PROVIDER_CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        provider_id: "anthropic",
        models: &["claude-opus-4-8"],
    },
    CatalogEntry {
        provider_id: "openai",
        models: &["gpt-5.5"],
    },
    CatalogEntry {
        provider_id: "deepseek",
        models: &["deepseek-v4-pro"],
    },
    CatalogEntry {
        provider_id: "wps",
        models: WPS_MODELS,
    },
];

pub fn models_for(id: &str) -> Option<&'static [&'static str]> {
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.provider_id == id)
        .map(|entry| entry.models)
}

#[cfg(test)]
mod tests {
    use super::models_for;

    #[test]
    fn models_for_wps_includes_zhipu_glm_5_2() {
        let models = models_for("wps").expect("wps catalog should exist");
        assert!(models.contains(&"zhipu/glm-5.2"));
        assert_eq!(models.len(), 8);
    }

    #[test]
    fn models_for_preset_providers_return_expected_singletons() {
        assert_eq!(models_for("anthropic"), Some(&["claude-opus-4-8"][..]));
        assert_eq!(models_for("openai"), Some(&["gpt-5.5"][..]));
        assert_eq!(models_for("deepseek"), Some(&["deepseek-v4-pro"][..]));
    }

    #[test]
    fn models_for_unknown_id_returns_none() {
        assert_eq!(models_for("my-llm"), None);
    }

    #[test]
    fn models_for_wps_and_deepseek_return_distinct_openai_kind_catalogs() {
        let wps = models_for("wps").unwrap();
        let deepseek = models_for("deepseek").unwrap();
        assert!(wps.iter().any(|model| model.starts_with("zhipu/")));
        assert!(!wps.iter().any(|model| model.starts_with("gpt-")));
        assert_eq!(deepseek, &["deepseek-v4-pro"]);
        assert!(!deepseek.iter().any(|model| model.starts_with("gpt-")));
    }
}
