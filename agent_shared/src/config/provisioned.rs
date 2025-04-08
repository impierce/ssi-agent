use identity_iota::{core::ToJson, storage::KeyId};
use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use oid4vp::ClaimFormatDesignation;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
use tracing::{info, warn};
use url::Url;

use crate::config::{
    redact, EventStoreType, Events, LogFormat, Logo, SupportedDidMethod, ED25519_KEY_ID, ES256_KEY_ID,
};

/// Provisioned configuration values are immutable and protected against runtime modifications.
#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct ProvisionedApplicationConfiguration {
    pub port: Option<u16>,
    pub log_format: Option<LogFormat>,
    pub event_store: Option<EventStoreConfig>,
    pub url: Option<Url>,
    pub base_path: Option<String>,
    pub cors_enabled: Option<bool>,
    pub did_methods: Option<HashMap<SupportedDidMethod, ToggleOptions>>,
    pub external_server_response_timeout_ms: Option<u64>,
    pub domain_linkage_enabled: Option<bool>,
    pub credential_offer_by_value_enabled: Option<bool>,
    pub secret_manager: Option<SecretManagerConfig>,
    pub credential_configurations: Option<Vec<CredentialConfiguration>>, // TODO: pay attention to index when merging provisioned and runtime credential_configurations!
    pub signing_algorithms_supported: Option<HashMap<jsonwebtoken::Algorithm, ToggleOptions>>,
    pub display: Option<Vec<Display>>,
    pub event_publishers: Option<EventPublishers>,
    pub vp_formats: Option<HashMap<ClaimFormatDesignation, ToggleOptions>>,
}

/// Loads provisioned configuration from a yaml file and environment variables.
pub fn load_provisioned_config() -> Result<ProvisionedApplicationConfiguration, config::ConfigError> {
    let mut builder = config::Config::builder();
    let config_file_path = if cfg!(feature = "test_utils") {
        "../agent_shared/tests/test.config.yaml".to_string()
    } else {
        std::env::var("UNICORE__CONFIG_FILE").unwrap_or_else(|_| "./config.yaml".to_string())
    };

    if std::path::Path::new(&config_file_path).exists() {
        builder = builder.add_source(config::File::with_name(&config_file_path));
        println!("Loaded config file: {}", config_file_path);
        info!("Loaded config file: {}", config_file_path);
    } else {
        println!("Config file not found: {}", config_file_path);
        warn!("Config file not found: {}", config_file_path);
    }

    if cfg!(feature = "test_utils") {
        builder = builder.add_source(config::Environment::with_prefix("TEST_UNICORE").separator("__"))
    } else {
        builder = builder.add_source(config::Environment::with_prefix("UNICORE").separator("__"))
    };

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

impl Into<crate::config::SecretManagerConfig> for SecretManagerConfig {
    fn into(self) -> crate::config::SecretManagerConfig {
        crate::config::SecretManagerConfig {
            stronghold_path: self.stronghold_path.unwrap_or(STRONGHOLD_PATH.to_string()),
            stronghold_password: self.stronghold_password,
            issuer_eddsa_key_id: self.issuer_eddsa_key_id.unwrap_or(KeyId::new(ED25519_KEY_ID)),
            issuer_es256_key_id: self.issuer_es256_key_id.unwrap_or(KeyId::new(ES256_KEY_ID)),
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventStoreConfig {
    #[serde(rename = "type")]
    pub type_: Option<EventStoreType>,
    #[serde(serialize_with = "redact")]
    pub connection_string: Option<String>, // TODO: consider making this "env-only", not via config file
}

impl Into<crate::config::EventStoreConfig> for EventStoreConfig {
    fn into(self) -> crate::config::EventStoreConfig {
        crate::config::EventStoreConfig {
            type_: self.type_.unwrap_or_default(),
            connection_string: self.connection_string,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct ToggleOptions {
    pub enabled: Option<bool>,
    pub preferred: Option<bool>,
}

impl Into<crate::config::ToggleOptions> for ToggleOptions {
    fn into(self) -> crate::config::ToggleOptions {
        crate::config::ToggleOptions {
            enabled: self.enabled.unwrap_or_default(),
            preferred: self.preferred,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct Display {
    pub name: Option<String>,
    pub locale: Option<String>,
    pub logo: Option<Logo>,
}

impl Into<crate::config::Display> for Display {
    fn into(self) -> crate::config::Display {
        crate::config::Display {
            name: self.name.unwrap_or_default(),
            locale: self.locale,
            logo: self.logo,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct CredentialConfiguration {
    pub credential_configuration_id: Option<String>,
    #[serde(flatten)]
    pub credential_format_with_parameters: Option<CredentialFormats<WithParameters>>,
    #[serde(default)]
    pub display: Option<Vec<serde_json::Value>>,
}

impl Into<crate::config::CredentialConfiguration> for CredentialConfiguration {
    fn into(self) -> crate::config::CredentialConfiguration {
        crate::config::CredentialConfiguration {
            credential_configuration_id: self.credential_configuration_id.unwrap_or_default(),
            credential_format_with_parameters: self.credential_format_with_parameters.unwrap_or_default().into(),
            display: self.display.unwrap_or_default(),
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventPublishers {
    pub http: Option<EventPublisherHttp>,
}

impl Into<crate::config::EventPublishers> for EventPublishers {
    fn into(self) -> crate::config::EventPublishers {
        crate::config::EventPublishers {
            http: self.http.map(Into::into),
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventPublisherHttp {
    pub enabled: Option<bool>,
    pub target_url: Option<String>,
    #[serde(with = "http_serde::option::header_map", default)]
    pub headers: Option<reqwest::header::HeaderMap>,
    pub events: Option<Events>,
}

impl Into<crate::config::EventPublisherHttp> for EventPublisherHttp {
    fn into(self) -> crate::config::EventPublisherHttp {
        crate::config::EventPublisherHttp {
            enabled: self.enabled.unwrap_or_default(),
            target_url: self.target_url.unwrap_or_default(),
            headers: self.headers,
            events: self.events.unwrap_or_default().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_config_file_not_found_returns_empty_config() {
        temp_env::with_var("UNICORE__CONFIG_FILE", Some("./config.yaml"), || {
            let config = load_provisioned_config().unwrap();

            let serialized = serde_json::to_value(&config).unwrap();

            assert_eq!(serialized, json!({}));
        });
    }

    #[test]
    #[serial]
    fn test_loads_config_file() {
        temp_env::with_var(
            "UNICORE__CONFIG_FILE",
            Some("../agent_application/example.config.yaml"),
            || {
                let config = load_provisioned_config().unwrap();

                let serialized = serde_json::to_value(&config).unwrap();

                println!("{}", serde_json::to_string_pretty(&serialized).unwrap());

                assert_eq!(serialized.get("url").unwrap(), &json!("https://ssi-agent.example.org/"));
            },
        );
    }

    #[test]
    #[serial]
    fn test_env_var_overwrites_config_file() {
        let config = load_provisioned_config().unwrap();
        let serialized = serde_json::to_value(&config.log_format).unwrap();
        assert_eq!(serialized, json!("json"));

        // Set the environment variable to override the config file value
        temp_env::with_vars([("UNICORE__LOG_FORMAT", Some("text"))], || {
            let config = load_provisioned_config().unwrap();

            let serialized = serde_json::to_value(&config.log_format).unwrap();

            assert_eq!(serialized, json!("text"));
        })
    }
}
