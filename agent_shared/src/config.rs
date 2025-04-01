use config::ConfigError;
use identity_iota::storage::KeyId;
use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::{skip_serializing_none, SerializeDisplay};
use std::{
    collections::HashMap,
    sync::{RwLock, RwLockReadGuard},
};
use strum::VariantArray;
use tracing::{debug, info};
use url::Url;

static STRONGHOLD_PATH: &str = "./stronghold.dat";

// TODO: Once we have a proper state implementation for `agent_secret_manager` we can make use of randomly generated Key
// IDs. For now we need to make use of these static variables.
static ED25519_KEY_ID: &str = "ed25519-0";
static ES256_KEY_ID: &str = "es256-0";

#[derive(Debug, Deserialize, Clone)]
pub struct ApplicationConfiguration {
    pub log_format: LogFormat,
    pub event_store: EventStoreConfig,
    pub application_url: Url,
    pub application_base_path: Option<String>,
    pub public_url: Option<Url>,
    pub cors_enabled: Option<bool>,
    pub did_methods: HashMap<SupportedDidMethod, ToggleOptions>,
    pub external_server_response_timeout_ms: Option<u64>,
    pub domain_linkage_enabled: bool,
    pub credential_offer_by_value_enabled: Option<bool>,
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

#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
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
    #[serde(default = "default_stronghold_path")]
    pub stronghold_path: String,
    pub stronghold_password: String,
    #[serde(default = "default_issuer_eddsa_key_id")]
    pub issuer_eddsa_key_id: KeyId,
    #[serde(default = "default_issuer_es256_key_id")]
    pub issuer_es256_key_id: KeyId,
}

fn default_stronghold_path() -> String {
    STRONGHOLD_PATH.to_string()
}

pub fn default_issuer_eddsa_key_id() -> KeyId {
    KeyId::new(ED25519_KEY_ID)
}

pub fn default_issuer_es256_key_id() -> KeyId {
    KeyId::new(ES256_KEY_ID)
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
    #[serde(with = "http_serde::option::header_map", default)]
    pub headers: Option<reqwest::header::HeaderMap>,
    pub events: Events,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Events {
    #[serde(default)]
    pub connection: Vec<ConnectionEvent>,
    #[serde(default)]
    pub document: Vec<DocumentEvent>,
    #[serde(default)]
    pub service: Vec<ServiceEvent>,
    #[serde(default)]
    pub server_config: Vec<ServerConfigEvent>,
    #[serde(default)]
    pub credential: Vec<CredentialEvent>,
    #[serde(default)]
    pub offer: Vec<OfferEvent>,
    #[serde(default)]
    pub holder_credential: Vec<HolderCredentialEvent>,
    #[serde(default)]
    pub received_offer: Vec<ReceivedOfferEvent>,
    #[serde(default)]
    pub authorization_request: Vec<AuthorizationRequestEvent>,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum ConnectionEvent {
    ConnectionAdded,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum DocumentEvent {
    DocumentCreated,
    PublicKeyUpdated,
    DocumentStatusUpdated,
    ServiceAdded,
    DocumentPublished,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum ServiceEvent {
    DomainLinkageServiceCreated,
    DomainLinkageServiceDeleted,
    LinkedVerifiablePresentationServiceCreated,
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
pub enum HolderCredentialEvent {
    CredentialAdded,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum ReceivedOfferEvent {
    CredentialOfferReceived,
    CredentialOfferAccepted,
    TokenResponseReceived,
    CredentialResponseReceived,
    CredentialOfferRejected,
}

#[derive(Debug, Serialize, Deserialize, Clone, strum::Display)]
pub enum AuthorizationRequestEvent {
    AuthorizationRequestCreated,
    FormUrlEncodedAuthorizationRequestCreated,
    AuthorizationRequestObjectSigned,
    SIOPv2AuthorizationResponseVerified,
    OID4VPAuthorizationResponseVerified,
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
    Debug,
    Deserialize,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    strum::EnumString,
    strum::Display,
    SerializeDisplay,
    Ord,
    PartialOrd,
    VariantArray,
)]
pub enum SupportedDidMethod {
    #[serde(alias = "did_jwk", alias = "did:jwk", rename = "did_jwk")]
    #[strum(serialize = "did:jwk")]
    Jwk,
    #[serde(alias = "did_key", alias = "did:key", rename = "did_key")]
    #[strum(serialize = "did:key")]
    Key,
    #[serde(alias = "did_web", alias = "did:web", rename = "did_web")]
    #[strum(serialize = "did:web")]
    Web,
    #[serde(alias = "did_iota", alias = "did:iota", rename = "did_iota")]
    #[strum(serialize = "did:iota")]
    Iota,
    #[serde(alias = "did_iota_smr", alias = "did:iota:smr", rename = "did_iota_smr")]
    #[strum(serialize = "did:iota:smr")]
    IotaSmr,
}

/// (A subset of) DID method traits. The methods follow a naming convention that expresses boolean predicates as verb
/// phrases, as specified in the DID traits documentation:
/// https://github.com/decentralized-identity/did-traits/blob/v0.8.0/schemas/v0.8.0/traits.json
impl SupportedDidMethod {
    pub fn supports_update(&self) -> bool {
        match self {
            SupportedDidMethod::Web | SupportedDidMethod::Iota | SupportedDidMethod::IotaSmr => true,
            SupportedDidMethod::Jwk | SupportedDidMethod::Key => false,
        }
    }

    pub fn hosted_centrally(&self) -> bool {
        match self {
            SupportedDidMethod::Jwk
            | SupportedDidMethod::Key
            | SupportedDidMethod::Iota
            | SupportedDidMethod::IotaSmr => false,
            SupportedDidMethod::Web => true,
        }
    }

    pub fn hosted_decentrally(&self) -> bool {
        match self {
            SupportedDidMethod::Jwk | SupportedDidMethod::Key | SupportedDidMethod::Web => false,
            SupportedDidMethod::Iota | SupportedDidMethod::IotaSmr => true,
        }
    }
}

const MAINNET_URL: &str = "https://api.stardust-mainnet.iotaledger.net";
const SHIMMER_URL: &str = "https://api.shimmer.network";

const IOTA_NETWORK: &str = "IOTA Network";
const SHIMMER_NETWORK: &str = "Shimmer Network";

// See specification: "Since did:jwk only contains a single key, the DID URL fragment identifier is always a fixed #0 value."
const JWK_FRAGMENT: &str = "0";

impl SupportedDidMethod {
    pub fn api_endpoint(&self) -> Option<&str> {
        match self {
            SupportedDidMethod::Iota => Some(MAINNET_URL),
            SupportedDidMethod::IotaSmr => Some(SHIMMER_URL),
            SupportedDidMethod::Jwk | SupportedDidMethod::Key | SupportedDidMethod::Web => None,
        }
    }

    pub fn network_name(&self) -> Option<&str> {
        match self {
            SupportedDidMethod::Iota => Some(IOTA_NETWORK),
            SupportedDidMethod::IotaSmr => Some(SHIMMER_NETWORK),
            SupportedDidMethod::Jwk | SupportedDidMethod::Key | SupportedDidMethod::Web => None,
        }
    }

    pub fn fragment(&self) -> Option<&str> {
        match self {
            SupportedDidMethod::Jwk => Some(JWK_FRAGMENT),
            SupportedDidMethod::Iota
            | SupportedDidMethod::IotaSmr
            | SupportedDidMethod::Key
            | SupportedDidMethod::Web => None,
        }
    }
}

impl From<SupportedDidMethod> for SubjectSyntaxType {
    fn from(val: SupportedDidMethod) -> Self {
        SubjectSyntaxType::try_from(val.to_string().as_str()).expect("conversion into `SubjectSyntaxType` failed")
    }
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

            if config.event_store.type_ == EventStoreType::InMemory {
                for did_method in &[SupportedDidMethod::Iota, SupportedDidMethod::IotaSmr] {
                    if config
                        .did_methods
                        .get(did_method)
                        .map(|options| options.enabled)
                        .unwrap_or_default()
                    {
                        panic!("`{did_method}` cannot be enabled when using the `in_memory` event store");
                    }
                }
            }
        })
    }

    pub fn set_preferred_did_method(&mut self, preferred_did_method: SupportedDidMethod) {
        // Set the current preferred did_method to false if available.
        if let Some((_, options)) = self.did_methods.iter_mut().find(|(_, v)| v.preferred == Some(true)) {
            options.preferred = Some(false);
        }

        // Set the current preferred did_method to true.
        let entry = self
            .did_methods
            .entry(preferred_did_method)
            .or_insert_with(|| ToggleOptions {
                enabled: true,
                preferred: Some(true),
            });
        entry.enabled = true;
        entry.preferred = Some(true);
    }

    pub fn disable_did_method(&mut self, did_method: SupportedDidMethod) {
        if let Some(options) = self.did_methods.get_mut(&did_method) {
            options.enabled = false;
        }
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
        .map(|(k, _)| *k)
        .collect();

    did_methods.sort();

    did_methods
}

// TODO: should fail when none is enabled
pub fn get_all_enabled_signing_algorithms_supported() -> Vec<Algorithm> {
    let mut signing_algorithms_supported: Vec<_> = config()
        .signing_algorithms_supported
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| *k)
        .collect();

    // `jsonwebtoken::Algorithm` does not implement `Display` so we need to serialize it through `serde_json` first in
    // order to sort.
    signing_algorithms_supported.sort_by(|a, b| json!(a).as_str().cmp(&json!(b).as_str()));

    signing_algorithms_supported
}

// TODO: should fail when there's more than one result
pub fn get_preferred_did_method() -> SupportedDidMethod {
    config()
        .did_methods
        .iter()
        .filter(|(_, v)| v.enabled)
        .filter(|(_, v)| v.preferred.unwrap_or(false))
        .map(|(k, _)| *k)
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

/// Returns the public URL if it is set, otherwise the application URL.
pub fn get_public_url() -> Url {
    config().public_url.clone().unwrap_or_else(|| {
        get_application_base_path()
            .ok()
            .as_ref()
            .and_then(|base_path| config().application_url.join(base_path).ok())
            .unwrap_or_else(|| config().application_url.clone())
    })
}

pub fn get_application_base_path() -> Result<String, ConfigError> {
    config()
        .application_base_path
        .clone()
        .ok_or_else(|| ConfigError::NotFound("No configuration for `application_base_path` found".to_string()))
        .map(|mut application_base_path| {
            if application_base_path.starts_with('/') {
                application_base_path.remove(0);
            }

            if application_base_path.ends_with('/') {
                application_base_path.pop();
            }

            if application_base_path.is_empty() {
                panic!("UNICORE__APPLICATION_BASE_PATH can't be empty, remove or set path");
            }

            info!("Application base path: {:?}", application_base_path);

            format!("/{}/", application_base_path)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_supported_did_methods_can_be_converted_into_subject_syntax_type() {
        for variant in SupportedDidMethod::VARIANTS {
            let _subject_syntax_type: SubjectSyntaxType = (*variant).into();
        }
    }
}
