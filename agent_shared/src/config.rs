use config::ConfigError;
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

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
    pub vp_formats: HashMap<ClaimFormatDesignation, ToggleOptions>,
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
    pub events: Events,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Events {
    pub server_config: Option<Vec<ServerConfigEvent>>,
    pub credential: Option<Vec<CredentialEvent>>,
    pub offer: Option<Vec<OfferEvent>>,
    pub connection: Option<Vec<ConnectionEvent>>,
    pub authorization_request: Option<Vec<AuthorizationRequestEvent>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum ServerConfigEvent {
    ServerMetadataInitialized,
    CredentialConfigurationAdded,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum CredentialEvent {
    UnsignedCredentialCreated,
    SignedCredentialCreated,
    CredentialSigned,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum OfferEvent {
    CredentialOfferCreated,
    CredentialsAdded,
    FormUrlEncodedCredentialOfferCreated,
    TokenResponseCreated,
    CredentialRequestVerified,
    CredentialResponseCreated,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum ConnectionEvent {
    SIOPv2AuthorizationResponseVerified,
    OID4VPAuthorizationResponseVerified,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum AuthorizationRequestEvent {
    AuthorizationRequestCreated,
    FormUrlEncodedAuthorizationRequestCreated,
    AuthorizationRequestObjectSigned,
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
pub fn config() -> ApplicationConfiguration {
    CONFIG.get_or_init(|| ApplicationConfiguration::new().unwrap()).clone()
    // TODO: or return -> &'static ApplicationConfiguration, so we don't need to clone on every call
}

pub fn did_methods_enabled() -> Vec<String> {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| k.clone().replace("_", ":"))
        .collect()
}

pub fn did_method_preferred() -> String {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .filter(|(_, v)| v.preferred.unwrap_or(false))
        .map(|(k, _)| k.clone().replace("_", ":"))
        .collect()
}
