use config::ConfigError;
use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_with::{skip_serializing_none, SerializeDisplay};
use std::{
    collections::HashMap,
    sync::{RwLock, RwLockReadGuard},
};
use tracing::{debug, info};
use url::Url;

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfiguration {
    pub log_format: LogFormat,
    pub event_store: EventStoreConfig,
    pub url: String,
    pub base_path: Option<String>,
    pub cors_enabled: Option<bool>,
    pub did_methods: HashMap<SupportedDidMethod, ToggleOptions>,
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
    // Postgres(EventStorePostgresConfig), // <== TODO: "config-rs" panics with "unreachable code", other solution?
    Postgres,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventStorePostgresConfig {
    pub connection_string: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecretManagerConfig {
    #[serde(default)]
    pub generate_stronghold: bool,
    pub stronghold_path: String,
    pub stronghold_password: String,
    pub issuer_eddsa_key_id: Option<String>,
    pub issuer_es256_key_id: Option<String>,
    pub issuer_did: Option<String>,
    pub issuer_fragment: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CredentialConfiguration {
    pub credential_configuration_id: String,
    #[serde(flatten)]
    pub credential_format_with_parameters: CredentialFormats<WithParameters>,
    #[serde(default)]
    pub display: Vec<serde_json::Value>,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Logo {
    pub uri: Option<Url>,
    pub alt_text: Option<String>,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Display {
    pub name: String,
    pub locale: Option<String>,
    pub logo: Option<Logo>,
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

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Events {
    #[serde(default)]
    pub server_config: Vec<ServerConfigEvent>,
    #[serde(default)]
    pub credential: Vec<CredentialEvent>,
    #[serde(default)]
    pub offer: Vec<OfferEvent>,
    #[serde(default)]
    pub connection: Vec<ConnectionEvent>,
    #[serde(default)]
    pub authorization_request: Vec<AuthorizationRequestEvent>,
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
/// ```
/// use agent_shared::config::SupportedDidMethod;
/// use serde_json::json;
///
/// let supported_did_method: SupportedDidMethod = serde_json::from_value(json!("did_jwk")).unwrap();
/// assert_eq!(supported_did_method, SupportedDidMethod::Jwk);
/// assert_eq!(supported_did_method.to_string(), "did:jwk");
/// ```
#[derive(
    Debug, Deserialize, Clone, Eq, PartialEq, Hash, strum::EnumString, strum::Display, SerializeDisplay, Ord, PartialOrd,
)]
pub enum SupportedDidMethod {
    #[serde(alias = "did_jwk", rename = "did_jwk")]
    #[strum(serialize = "did:jwk")]
    Jwk,
    #[serde(alias = "did_key", rename = "did_key")]
    #[strum(serialize = "did:key")]
    Key,
    #[serde(alias = "did_web", rename = "did_web")]
    #[strum(serialize = "did:web")]
    Web,
    #[serde(alias = "did_iota", rename = "did_iota")]
    #[strum(serialize = "did:iota")]
    Iota,
    #[serde(alias = "did_iota_smr", rename = "did_iota_smr")]
    #[strum(serialize = "did:iota:smr")]
    IotaSmr,
    #[serde(alias = "did_iota_rms", rename = "did_iota_rms")]
    #[strum(serialize = "did:iota:rms")]
    IotaRms,
}

/// Generic options that add an "enabled" field and a "preferred" field (optional) to a configuration.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ToggleOptions {
    pub enabled: bool,
    pub preferred: Option<bool>,
}

pub static CONFIG: Lazy<RwLock<ApplicationConfiguration>> =
    Lazy::new(|| RwLock::new(ApplicationConfiguration::new().unwrap()));

impl ApplicationConfiguration {
    pub fn new() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        // TODO: these cannot be logged because `tracing_subscriber` is not initialized yet at this point since it does
        // not know the log format yet.
        info!("Environment variables loaded.");
        info!("Loading application configuration ...");

        println!("Current directory: {:?}", std::env::current_dir().unwrap());

        let config = if cfg!(feature = "test_utils") {
            config::Config::builder()
                .add_source(config::File::with_name("../agent_shared/tests/test-config.yaml"))
                // TODO: other prefix for tests
                .add_source(config::Environment::with_prefix("TEST_UNICORE").separator("__"))
                .build()?
        } else {
            config::Config::builder()
                .add_source(config::File::with_name("agent_application/config.yaml"))
                .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                .build()?
        };

        config.try_deserialize().inspect(|config: &ApplicationConfiguration| {
            // TODO: this won't be logged either because `tracing_subscriber` is not initialized yet at this point. To
            // fix this we can consider obtaining the `log_format` from the config file prior to loading the complete
            // configuration.
            info!("Configuration loaded successfully");
            debug!("{:#?}", config);
        })
    }

    pub fn set_preferred_did_method(&mut self, preferred_did_method: SupportedDidMethod) {
        // Set the current preferred did_method to false if available.
        if let Some((_, options)) = self.did_methods.iter_mut().find(|(_, v)| v.preferred == Some(true)) {
            options.preferred = Some(false);
        }

        // Set the current preferred did_method to true if available.
        self.did_methods
            .entry(preferred_did_method)
            .or_insert_with(|| ToggleOptions {
                enabled: true,
                preferred: Some(true),
            })
            .preferred = Some(true);
    }

    // TODO: make generic: set_enabled(enabled: bool)
    pub fn enable_event_publisher_http(&mut self) {
        if let Some(event_publishers) = &mut self.event_publishers {
            if let Some(http) = &mut event_publishers.http {
                http.enabled = true;
            }
        }
    }

    pub fn set_event_publisher_http_target_url(&mut self, target_url: String) {
        if let Some(event_publishers) = &mut self.event_publishers {
            if let Some(http) = &mut event_publishers.http {
                http.target_url = target_url;
            }
        }
    }

    pub fn set_event_publisher_http_target_events(&mut self, events: Events) {
        if let Some(event_publishers) = &mut self.event_publishers {
            if let Some(http) = &mut event_publishers.http {
                http.events = events;
            }
        }
    }

    pub fn set_secret_manager_config(&mut self, config: SecretManagerConfig) {
        self.secret_manager = config;
    }
}

/// Returns the application configuration or loads it, if it hasn't been loaded already.
pub fn config<'a>() -> RwLockReadGuard<'a, ApplicationConfiguration> {
    CONFIG.read().unwrap()
}

/// Returns Write Guard for the application configuration that can be used to update the configuration during runtime.
#[cfg(feature = "test_utils")]
pub fn set_config<'a>() -> std::sync::RwLockWriteGuard<'a, ApplicationConfiguration> {
    CONFIG.write().unwrap()
}

// TODO: should fail when none is enabled
pub fn get_all_enabled_did_methods() -> Vec<SupportedDidMethod> {
    let mut did_methods: Vec<_> = config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| k.clone())
        .collect();

    did_methods.sort();

    did_methods
}

// TODO: should fail when there's more than one result
pub fn get_preferred_did_method() -> SupportedDidMethod {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .filter(|(_, v)| v.preferred.unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect::<Vec<SupportedDidMethod>>()
        .first()
        .cloned()
        .expect("Please set a DID method as `preferred` in the configuration")
}

pub fn get_preferred_signing_algorithm() -> jsonwebtoken::Algorithm {
    config()
        .signing_algorithms_supported
        .iter()
        .filter(|(_, v)| v.enabled)
        .filter(|(_, v)| v.preferred.unwrap_or(false))
        .map(|(k, _)| *k)
        .collect::<Vec<jsonwebtoken::Algorithm>>()
        .first()
        .cloned()
        .expect("Please set a signing algorithm as `preferred` in the configuration")
}
