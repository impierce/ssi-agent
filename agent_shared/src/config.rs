use config::ConfigError;
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Mutex};
use tracing::info;

use crate::{issuance::CredentialConfiguration, metadata::Display};

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfiguration {
    pub log_format: LogFormat,
    pub event_store: EventStoreConfig,
    pub url: String,
    pub base_path: Option<String>,
    pub cors_enabled: Option<bool>,
    pub did_methods: HashMap<DidMethod, ToggleOptions>,
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

/// All DID methods supported by UniCore
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum DidMethod {
    #[serde(rename = "did:jwk", alias = "did_jwk")]
    Jwk,
    #[serde(rename = "did:key")]
    Key,
    #[serde(rename = "did:web")]
    Web,
    #[serde(rename = "did:iota:rms")]
    IotaRms,
}

/// Generic options that add an "enabled" field and a "preferred" field (optional) to a configuration.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ToggleOptions {
    pub enabled: bool,
    pub preferred: Option<bool>,
}

// pub static CONFIG: OnceCell<ApplicationConfiguration> = OnceCell::new();

pub static CONFIG: Lazy<Mutex<ApplicationConfiguration>> =
    Lazy::new(|| Mutex::new(ApplicationConfiguration::new().unwrap()));

impl ApplicationConfiguration {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        info!("Environment variables loaded.");
        info!("Loading application configuration ...");

        println!("Current directory: {:?}", std::env::current_dir().unwrap());

        let config = if cfg!(feature = "test_utils") {
            config::Config::builder()
                .add_source(config::File::with_name("../agent_shared/tests/test-config.yaml"))
                // TODO: other prefix for tests
                .add_source(config::Environment::with_prefix("TEST_AGENT").separator("__"))
                .build()?
        } else {
            config::Config::builder()
                .add_source(config::File::with_name("agent_application/example-config.yaml"))
                .add_source(config::Environment::with_prefix("AGENT").separator("__"))
                .build()?
        };

        config.try_deserialize()
    }
}

/// Returns the application configuration or loads it, if it hasn't been loaded already.
pub fn config() -> ApplicationConfiguration {
    // CONFIG.get_or_init(|| ApplicationConfiguration::new().unwrap()).clone()
    CONFIG.lock().unwrap().clone()
    // TODO: or return -> &'static ApplicationConfiguration, so we don't need to clone on every call
}

/// Reloads the config. Useful for testing after overwriting a env variable.
pub fn reload_config() {
    // CONFIG.set(ApplicationConfiguration::new().unwrap()).unwrap();
    // CONFIG.lock().unwrap().clone()
}

// TODO: should fail when none is enabled
pub fn get_all_enabled_did_methods() -> Vec<DidMethod> {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| k.clone())
        .collect()
}

// TODO: should fail when there's more than one result
pub fn get_preferred_did_method() -> DidMethod {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .filter(|(_, v)| v.preferred.unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect::<Vec<DidMethod>>()
        .first()
        .cloned()
        .expect("Please set a DID method as `preferred` in the configuration")
}

pub fn set_preferred_did_method() {}
