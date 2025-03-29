use identity_iota::storage::KeyId;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use url::Url;

use crate::config::{redact, EventStoreType, LogFormat};

/// Provisioned configuration values are immutable and protected against runtime modifications.
#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct ProvisionedApplicationConfiguration {
    pub log_format: Option<LogFormat>,
    pub event_store: Option<EventStoreConfig>,
    pub url: Option<Url>,
    pub base_path: Option<String>,
    pub cors_enabled: Option<bool>,
    // pub did_methods: HashMap<SupportedDidMethod, ToggleOptions>,
    pub external_server_response_timeout_ms: Option<u64>,
    pub domain_linkage_enabled: Option<bool>,
    pub credential_offer_by_value_enabled: Option<bool>,
    pub secret_manager: Option<SecretManagerConfig>,
    // pub credential_configurations: Vec<CredentialConfiguration>,
    // pub signing_algorithms_supported: HashMap<jsonwebtoken::Algorithm, ToggleOptions>,
    // pub display: Vec<Display>,
    // pub event_publishers: Option<EventPublishers>,
    // pub vp_formats: HashMap<ClaimFormatDesignation, ToggleOptions>,
}

/// Loads provisioned configuration from a yaml file and environment variables.
pub fn load_provisioned_config() -> Result<ProvisionedApplicationConfiguration, config::ConfigError> {
    let mut builder = config::Config::builder();
    let config_file_path = std::env::var("UNICORE__CONFIG_FILE").unwrap_or_else(|_| "./config.yaml".to_string());
    if std::path::Path::new(&config_file_path).exists() {
        builder = builder.add_source(config::File::with_name(&config_file_path));
    }
    builder = builder.add_source(config::Environment::with_prefix("UNICORE").separator("__"));
    let config = builder.build()?;

    config.try_deserialize::<ProvisionedApplicationConfiguration>()
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct SecretManagerConfig {
    pub stronghold_path: Option<String>,
    #[serde(serialize_with = "redact")]
    pub stronghold_password: Option<String>,
    pub issuer_eddsa_key_id: Option<KeyId>,
    pub issuer_es256_key_id: Option<KeyId>,
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventStoreConfig {
    #[serde(rename = "type")]
    pub type_: Option<EventStoreType>,
    #[serde(serialize_with = "redact")]
    pub connection_string: Option<String>, // TODO: consider making this "env-only", not via config file
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use serial_test::serial;

    static CONFIG_FILE: &str = "../agent_shared/tests/test.config.yaml";

    #[test]
    #[serial]
    fn test_no_values_provisioned_returns_empty_config() {
        let config = load_provisioned_config().unwrap();

        let serialized = serde_json::to_value(&config).unwrap();

        assert_eq!(serialized, json!({}));
    }

    #[test]
    #[serial]
    fn test_provisioned_env_var_is_used() {
        temp_env::with_var("UNICORE__LOG_FORMAT", Some("json"), || {
            let config = load_provisioned_config().unwrap();

            let serialized = serde_json::to_value(&config).unwrap();

            assert_eq!(
                serialized,
                json!({
                    "log_format": "json"
                })
            );
        });
    }

    #[test]
    #[serial]
    fn test_loads_config_file() {
        temp_env::with_var("UNICORE__CONFIG_FILE", Some(CONFIG_FILE), || {
            let config = load_provisioned_config().unwrap();

            let serialized = serde_json::to_value(&config).unwrap();

            assert_eq!(serialized, json!({}));
        });
    }

    #[test]
    #[serial]
    fn test_env_var_overwrites_config_file() {
        temp_env::with_vars(
            [
                ("UNICORE__CONFIG_FILE", Some(CONFIG_FILE)),
                ("UNICORE__LOG_FORMAT", Some("json")),
            ],
            || {
                let config = load_provisioned_config().unwrap();

                let serialized = serde_json::to_value(&config).unwrap();

                assert_eq!(
                    serialized,
                    json!({
                        "log_format": "json"
                    })
                );
            },
        )
    }
}
