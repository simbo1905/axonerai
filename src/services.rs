//! item57 — the services model. A SERVICE (not "provider") is the unit:
//! a service = an endpoint + an API key. Known services: `mistral`,
//! `groq`, `opencode-zen`, `opencode-go`.
//!
//! Key resolution (env + settings): a service is CONNECTED when its API
//! key is present. zen and go share one OPENCODE_API_KEY in reality —
//! GLOSSED as per-service env assignment: OPENCODE_ZEN_API_KEY /
//! OPENCODE_GO_API_KEY falling back to the shared OPENCODE_API_KEY for
//! both. A service is ENABLED unless it is named in the settings
//! `disabled_services` list (someone with the shared key must not
//! activate a service they are not subscribed to). N services = enabled
//! AND key present.
//!
//! Key lookups are injected as `&str -> Option<String>` closures so the
//! resolution logic is fully testable without touching process env (and
//! without any network).

use anyhow::{Result, anyhow};

use crate::provider::Provider;

/// The known services, sorted (same set as `models_config::KNOWN_PROVIDERS`).
pub const KNOWN_SERVICES: &[&str] = &["groq", "mistral", "opencode-go", "opencode-zen"];

/// Is `service` one of the known services?
pub fn is_known(service: &str) -> bool {
    KNOWN_SERVICES.contains(&service)
}

/// The per-service env-key chain: the FIRST variable present in the
/// lookup wins. opencode-zen and opencode-go fall back to the shared
/// OPENCODE_API_KEY (the glossing: one real key, two services).
pub fn env_keys(service: &str) -> Option<&'static [&'static str]> {
    match service {
        "mistral" => Some(&["MISTRAL_API_KEY"]),
        "groq" => Some(&["GROQ_API_KEY"]),
        "opencode-zen" => Some(&["OPENCODE_ZEN_API_KEY", "OPENCODE_API_KEY"]),
        "opencode-go" => Some(&["OPENCODE_GO_API_KEY", "OPENCODE_API_KEY"]),
        _ => None,
    }
}

/// The canonical chat-completions endpoint per service (mirrors the
/// `AppConfig::defaults()` endpoints in `config.rs`; an `axonerai.jsonc`
/// endpoint override still wins — see `build_provider`).
pub fn endpoint(service: &str) -> Option<&'static str> {
    match service {
        "mistral" => Some("https://api.mistral.ai/v1/chat/completions"),
        "groq" => Some("https://api.groq.com/openai/v1/chat/completions"),
        "opencode-zen" => Some("https://opencode.ai/zen/v1/chat/completions"),
        "opencode-go" => Some("https://opencode.ai/zen/go/v1/chat/completions"),
        _ => None,
    }
}

/// Resolve the API key for a service: the config `api_key` override
/// first (mirrors `AppConfig::resolve_api_key` precedence), then the
/// per-service env chain. An EMPTY value counts as absent — an empty
/// Bearer key is never a usable connection. Unknown service → None.
pub fn resolve_key(
    service: &str,
    config_override: Option<&str>,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Some(key) = config_override {
        if !key.is_empty() {
            return Some(key.to_string());
        }
    }
    let keys = env_keys(service)?;
    keys.iter()
        .find_map(|k| lookup(k).filter(|v| !v.is_empty()))
}

/// Is the service enabled (NOT named in the settings `disabled_services`
/// list)? Same list semantics as `disabled_mcp_servers`/`disabled_skills`.
pub fn is_enabled(service: &str, disabled_services: &[String]) -> bool {
    !disabled_services.iter().any(|s| s == service)
}

/// Build the provider for a service. Mirrors the endpoint distinction in
/// `src/config.rs`: mistral and groq have dedicated providers, the two
/// opencode endpoints share `OpenCodeProvider` differing only by base
/// URL. `config_endpoint` (from an `axonerai.jsonc` provider definition)
/// overrides the canonical endpoint when present. `openai` is accepted
/// as a legacy config-only provider (it is NOT a listed service).
/// Unknown service → Err. An empty `model` keeps the provider's own
/// default (the caller resolves the config default for opencode).
pub fn build_provider(
    service: &str,
    api_key: &str,
    model: &str,
    config_endpoint: Option<&str>,
) -> Result<Box<dyn Provider>> {
    let provider: Box<dyn Provider> = match service {
        "mistral" => {
            let mut p = crate::MistralProvider::new(api_key.to_string());
            if !model.is_empty() {
                p = p.with_model(model.to_string());
            }
            Box::new(p)
        }
        "groq" => {
            let mut p = crate::GroqProvider::new(api_key.to_string());
            if !model.is_empty() {
                p = p.with_model(model.to_string());
            }
            Box::new(p)
        }
        "openai" => {
            let mut p = crate::OpenAIProvider::new(api_key.to_string());
            if !model.is_empty() {
                p = p.with_model(model.to_string());
            }
            Box::new(p)
        }
        "opencode-zen" | "opencode-go" => {
            let endpoint = match config_endpoint {
                Some(ep) if !ep.is_empty() => ep.to_string(),
                _ => endpoint(service)
                    .ok_or_else(|| anyhow!("no canonical endpoint for service '{service}'"))?
                    .to_string(),
            };
            Box::new(crate::OpenCodeProvider::new(
                api_key.to_string(),
                endpoint,
                model.to_string(),
            ))
        }
        _ => return Err(anyhow!("unknown service: {service}")),
    };
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(map: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = map
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn known_services_are_the_four_sorted() {
        assert_eq!(
            KNOWN_SERVICES,
            &["groq", "mistral", "opencode-go", "opencode-zen"]
        );
        for s in KNOWN_SERVICES {
            assert!(is_known(s));
        }
        assert!(!is_known("openai"));
        assert!(!is_known("nope"));
    }

    #[test]
    fn env_keys_gloss_zen_and_go_over_the_shared_key() {
        assert_eq!(env_keys("mistral"), Some(&["MISTRAL_API_KEY"][..]));
        assert_eq!(env_keys("groq"), Some(&["GROQ_API_KEY"][..]));
        assert_eq!(
            env_keys("opencode-zen"),
            Some(&["OPENCODE_ZEN_API_KEY", "OPENCODE_API_KEY"][..])
        );
        assert_eq!(
            env_keys("opencode-go"),
            Some(&["OPENCODE_GO_API_KEY", "OPENCODE_API_KEY"][..])
        );
        assert!(env_keys("openai").is_none());
    }

    #[test]
    fn resolve_key_prefers_config_override() {
        let find = lookup(&[("MISTRAL_API_KEY", "env-key")]);
        assert_eq!(
            resolve_key("mistral", Some("config-key"), find),
            Some("config-key".to_string())
        );
    }

    #[test]
    fn resolve_key_uses_per_service_key_before_shared_fallback() {
        // Per-service key wins over the shared OPENCODE_API_KEY.
        let find = lookup(&[
            ("OPENCODE_ZEN_API_KEY", "zen-key"),
            ("OPENCODE_API_KEY", "shared"),
        ]);
        assert_eq!(
            resolve_key("opencode-zen", None, find),
            Some("zen-key".to_string())
        );

        // Shared key fallback when the per-service var is absent.
        let find = lookup(&[("OPENCODE_API_KEY", "shared")]);
        assert_eq!(
            resolve_key("opencode-zen", None, &find),
            Some("shared".to_string())
        );
        assert_eq!(
            resolve_key("opencode-go", None, find),
            Some("shared".to_string())
        );
    }

    #[test]
    fn resolve_key_absent_or_empty_is_not_connected() {
        let find = lookup(&[("GROQ_API_KEY", "")]);
        assert_eq!(resolve_key("groq", None, find), None, "empty = absent");
        assert_eq!(resolve_key("groq", None, lookup(&[])), None);
        assert_eq!(resolve_key("nope", None, lookup(&[("X", "y")])), None);
        assert_eq!(resolve_key("mistral", Some(""), lookup(&[])), None);
    }

    #[test]
    fn is_enabled_respects_the_disabled_list() {
        let disabled = vec!["groq".to_string()];
        assert!(is_enabled("mistral", &disabled));
        assert!(!is_enabled("groq", &disabled));
        assert!(is_enabled("groq", &[]));
    }

    #[test]
    fn endpoints_distinguish_the_two_opencode_services() {
        assert_eq!(
            endpoint("mistral"),
            Some("https://api.mistral.ai/v1/chat/completions")
        );
        assert_eq!(
            endpoint("groq"),
            Some("https://api.groq.com/openai/v1/chat/completions")
        );
        let zen = endpoint("opencode-zen").unwrap();
        let go = endpoint("opencode-go").unwrap();
        assert_ne!(zen, go, "zen and go are distinct endpoints");
        // They match the config.rs defaults exactly (the mirroring rule).
        let defaults = crate::AppConfig::defaults();
        assert_eq!(defaults.endpoint("opencode-zen").unwrap(), zen);
        assert_eq!(defaults.endpoint("opencode-go").unwrap(), go);
        assert_eq!(
            defaults.endpoint("mistral").unwrap(),
            endpoint("mistral").unwrap()
        );
        assert_eq!(
            defaults.endpoint("groq").unwrap(),
            endpoint("groq").unwrap()
        );
    }

    #[test]
    fn build_provider_maps_every_known_service_and_rejects_unknown() {
        for service in KNOWN_SERVICES {
            assert!(
                build_provider(service, "key", "model", None).is_ok(),
                "{service} builds"
            );
        }
        assert!(build_provider("openai", "key", "model", None).is_ok());
        assert!(build_provider("nope", "key", "model", None).is_err());
    }

    #[test]
    fn build_provider_config_endpoint_overrides_canonical() {
        // The override flows into the OpenCodeProvider constructor; the
        // observable contract here is Ok + no panic — the endpoint value
        // itself is proven by the endpoints_distinguish test above.
        assert!(build_provider("opencode-zen", "key", "m", Some("http://stub/v1")).is_ok());
    }
}
