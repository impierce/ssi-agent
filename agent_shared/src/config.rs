use config::ConfigError;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

use crate::{issuance::CredentialConfiguration, metadata::Display};

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfiguration {
    pub log_format: LogFormat,
    pub event_store: EventStoreConfig,
    pub url: String,
    pub base_path: Option<String>,
    pub cors_enabled: Option<bool>,
    pub did_methods: HashMap<String, ToggleOptions>,
    pub external_server_response_timeout_ms: Option<u64>,
    pub domain_linkage_enabled: bool,
    pub secret_manager: SecretManagerConfig,
    pub credential_configurations: Vec<CredentialConfiguration>,
    pub signing_algorithms_supported: HashMap<jsonwebtoken::Algorithm, ToggleOptions>,
    pub display: Vec<Display>,
    pub event_publishers: Option<EventPublishers>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Text,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventStoreConfig {
    #[serde(rename = "type")]
    pub type_: EventStoreType,
    pub connection_string: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum EventStoreType {
    InMemory,
    // Postgres(EventStorePostgresConfig), // <== TODO: "config-rs" panicks with "unreachable code", other solution?
    Postgres,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventStorePostgresConfig {
    pub connection_string: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecretManagerConfig {
    pub stronghold_path: String,
    pub stronghold_password: String,
    pub issuer_key_id: Option<String>,
    pub issuer_did: Option<String>,
    pub issuer_fragment: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventPublishers {
    pub http: Option<EventPublisherHttp>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventPublisherHttp {
    pub enabled: bool,
    pub target_url: String,
    pub events: Vec<Event>,
}

/// All events that can be published.
#[derive(Debug, Serialize, Deserialize, Clone, Hash, Eq, PartialEq)]
pub enum Event {
    // credential
    UnsignedCredentialCreated,
    SignedCredentialCreated,
    CredentialSigned,
    // offer
    CredentialOfferCreated,
    CredentialAdded,
    FormUrlEncodedCredentialOfferCreated,
    TokenResponseCreated,
    CredentialRequestVerified,
    CredentialResponseCreated,
    // server_config
    ServerMetadataLoaded,
    CredentialConfigurationAdded,
    // authorization_request
    AuthorizationRequestCreated,
    FormUrlEncodedAuthorizationRequestCreated,
    AuthorizationRequestObjectSigned,
    // connection
    SIOPv2AuthorizationResponseVerified,
    OID4VPAuthorizationResponseVerified,
}

// pub enum DidMethod {
//     #[serde(rename = "did:jwk")]
//     Jwk,
//     Key,
//     Web,
//     IotaRms,
// }

/// Generic options that add an "enabled" field and a "preferred" field (optional) to a configuration.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ToggleOptions {
    pub enabled: bool,
    pub preferred: Option<bool>,
}

static CONFIG: OnceCell<ApplicationConfiguration> = OnceCell::new();

impl ApplicationConfiguration {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        info!("Environment variables loaded.");
        info!("Loading application configuration ...");
        let config = config::Config::builder()
            .add_source(config::File::with_name("agent_application/example-config.yaml"))
            .add_source(config::Environment::with_prefix("AGENT").separator("__"))
            .build()?;
        config.try_deserialize()
    }
}

/// Returns the application configuration or loads it, if it hasn't been loaded already.
pub fn config_2() -> ApplicationConfiguration {
    // info!("config_2()");
    CONFIG.get_or_init(|| ApplicationConfiguration::new().unwrap()).clone()
    // TODO: or return -> &'static ApplicationConfiguration, so we don't need to clone on every call
}

/// Read environment variables
#[allow(unused)]
pub fn config(package_name: &str) -> config::Config {
    #[cfg(feature = "test_utils")]
    let config = test_config();

    #[cfg(not(feature = "test_utils"))]
    let config = {
        dotenvy::dotenv().ok();

        config::Config::builder()
            // .add_source(config::Environment::with_prefix(package_name))
            // TODO: read config.{run_mode}.yml from env "RUN_MODE"
            .add_source(config::File::with_name("agent_application/example-config.yaml"))
            .add_source(config::Environment::with_prefix("AGENT"))
            .build()
            .unwrap()
    };

    // TODO: this should ideally only printed once on startup or when the config changed
    // info!("{:#?}", config.clone().try_deserialize::<serde_yaml::Value>().unwrap());

    config
}

/// Read environment variables for tests that can be used across packages
#[cfg(feature = "test_utils")]
fn test_config() -> config::Config {
    use crate::{issuance::TEST_ISSUER_CONFIG, metadata::TEST_METADATA};
    use std::env;

    dotenvy::from_filename("agent_shared/tests/.env.test").ok();

    env::remove_var("AGENT_APPLICATION_BASE_PATH");

    let mut config_builder = config::Config::builder().add_source(config::Environment::with_prefix("TEST"));

    // If some test metadata configuration is set then add it to the global configuration.
    let metadata = TEST_METADATA.lock().unwrap();
    if let Some(metadata) = metadata.as_ref() {
        let metadata_string = serde_yaml::to_string(metadata).unwrap();
        config_builder = config_builder.add_source(config::File::from_str(&metadata_string, config::FileFormat::Yaml));
    }

    config_builder = config_builder.add_source(config::File::from_str(
        &serde_yaml::to_string(&*TEST_ISSUER_CONFIG).unwrap(),
        config::FileFormat::Yaml,
    ));

    config_builder.build().unwrap()
}
