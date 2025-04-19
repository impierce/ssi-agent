mod defaults;
mod provisioned;

use config::ConfigError;
use identity_iota::storage::KeyId;
use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::Lazy;
use provisioned::ProvisionedApplicationConfiguration;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::{skip_serializing_none, SerializeDisplay};
use std::{
    collections::HashMap,
    io::Write,
    sync::{RwLock, RwLockReadGuard},
};
use strum::VariantArray;
use tracing::{debug, info, warn};
use url::Url;

use crate::{error::SharedError, profile::ApplicationProfile};
// Re-export
pub use provisioned::load_provisioned_config;

static STRONGHOLD_PATH: &str = "./stronghold.dat";

// TODO: Once we have a proper state implementation for `agent_secret_manager` we can make use of randomly generated Key
// IDs. For now we need to make use of these static variables.
static ED25519_KEY_ID: &str = "ed25519-0";
static ES256_KEY_ID: &str = "es256-0";

#[serde_with::apply(
    Option => #[serde(skip_serializing_if = "Option::is_none")],
    Vec => #[serde(skip_serializing_if = "Vec::is_empty")],
    HashMap => #[serde(skip_serializing_if = "HashMap::is_empty")]
)]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct ApplicationConfiguration {
    pub port: Option<u16>,
    pub log_format: LogFormat,
    pub event_store: EventStoreConfig,
    pub url: Option<Url>,
    pub base_path: Option<String>,
    pub cors_enabled: bool,
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

#[derive(Debug, Deserialize, Clone, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Text,
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventStoreConfig {
    #[serde(rename = "type")]
    pub type_: EventStoreType,
    #[serde(serialize_with = "redact")]
    pub connection_string: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Eq, PartialEq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStoreType {
    InMemory,
    // Postgres(EventStorePostgresConfig), // <== TODO: "config-rs" panics with "unreachable code", other solution?
    #[default]
    Postgres,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EventStorePostgresConfig {
    pub connection_string: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct SecretManagerConfig {
    pub stronghold_path: String,
    #[serde(serialize_with = "redact")]
    pub stronghold_password: Option<String>,
    pub issuer_eddsa_key_id: KeyId,
    pub issuer_es256_key_id: KeyId,
}

impl Default for SecretManagerConfig {
    fn default() -> Self {
        SecretManagerConfig {
            stronghold_path: STRONGHOLD_PATH.to_string(),
            stronghold_password: None,
            issuer_eddsa_key_id: KeyId::new(ED25519_KEY_ID),
            issuer_es256_key_id: KeyId::new(ES256_KEY_ID),
        }
    }
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

#[derive(Debug, Deserialize, Clone, Serialize, Default)]
pub struct EventPublishers {
    pub http: Option<EventPublisherHttp>,
}

#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct EventPublisherHttp {
    pub enabled: bool,
    pub target_url: String,
    #[serde(with = "http_serde::option::header_map", default)]
    pub headers: Option<reqwest::header::HeaderMap>,
    pub events: Events,
}

#[derive(Debug, Deserialize, Clone, Default, Serialize)]
pub struct Events {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connection: Vec<ConnectionEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document: Vec<DocumentEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service: Vec<ServiceEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_config: Vec<ServerConfigEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential: Vec<CredentialEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offer: Vec<OfferEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holder_credential: Vec<HolderCredentialEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub received_offer: Vec<ReceivedOfferEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    NotificationReceived,
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
#[skip_serializing_none]
#[derive(Debug, Deserialize, Default, Clone, Serialize)]
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

        debug!("Current directory: {:?}", std::env::current_dir().unwrap());

        let mut default_config = ApplicationConfiguration::default();

        let application_profile = ApplicationProfile::load();

        // Apply default values to the empty configuration according to the application profile.
        match application_profile {
            ApplicationProfile::Development => {
                default_config.apply_development_defaults();
                println!("Development profile loaded");
                info!("Development profile loaded");
            }
            ApplicationProfile::Production => {
                default_config.apply_production_defaults();
            }
        }

        let provisioned_config = load_provisioned_config()?;

        let merged_config = default_config.merge(provisioned_config);

        // If the application is running in production mode, the configuration is validated.
        match application_profile {
            ApplicationProfile::Production => merged_config
                .validate()
                .map_err(|e| ConfigError::Message(e.to_string()))?,
            _ => {}
        }

        // Log final configuration
        if cfg!(debug_assertions) {
            std::fs::create_dir("./debug").ok();
            let mut file = std::fs::File::create("./debug/config.yaml").unwrap();
            file.write_all("# THIS FILE WAS GENERATED. ANY CHANGES WILL BE OVERWRITTEN!\n".as_bytes())
                .unwrap();
            file.write_all(serde_yaml::to_string(&merged_config).unwrap().as_bytes())
                .unwrap();
        }

        return Ok(merged_config);

        // TODO: include this logic again

        // provisioned_config
        //     .try_deserialize()
        //     .inspect(|config: &ApplicationConfiguration| {
        //         // TODO: this won't be logged either because `tracing_subscriber` is not initialized yet at this point. To
        //         // fix this we can consider obtaining the `log_format` from the config file prior to loading the complete
        //         // configuration.
        //         info!("Configuration loaded successfully");
        //         debug!("{:#?}", config);

        //         if config.event_store.type_ == EventStoreType::InMemory {
        //             for did_method in &[SupportedDidMethod::Iota, SupportedDidMethod::IotaSmr] {
        //                 if config
        //                     .did_methods
        //                     .get(did_method)
        //                     .map(|options| options.enabled)
        //                     .unwrap_or_default()
        //                 {
        //                     panic!("`{did_method}` cannot be enabled when using the `in_memory` event store");
        //                 }
        //             }
        //         }
        //     })
    }

    fn merge(&mut self, provisioned_config: ProvisionedApplicationConfiguration) -> Self {
        self.port = provisioned_config.port.and_then(|port| Some(port));
        self.log_format = provisioned_config.log_format.unwrap_or(self.clone().log_format);
        self.event_store = provisioned_config
            .event_store
            .map(|config| config.into())
            .unwrap_or(self.clone().event_store);
        self.url = provisioned_config.url.or(self.url.clone());
        self.base_path = provisioned_config.base_path.or(self.base_path.clone());
        self.cors_enabled = provisioned_config.cors_enabled.unwrap_or(self.cors_enabled);
        self.did_methods = provisioned_config
            .did_methods
            .map(|map| {
                map.into_iter()
                    .map(|(method, options)| (method, options.into()))
                    .collect()
            })
            .unwrap_or(self.did_methods.clone());
        self.external_server_response_timeout_ms = provisioned_config
            .external_server_response_timeout_ms
            .or(self.external_server_response_timeout_ms.clone());
        self.domain_linkage_enabled = provisioned_config
            .domain_linkage_enabled
            .unwrap_or(self.domain_linkage_enabled);
        self.credential_offer_by_value_enabled = provisioned_config
            .credential_offer_by_value_enabled
            .or(self.credential_offer_by_value_enabled.clone());
        self.secret_manager = provisioned_config
            .secret_manager
            .map(|config| config.into())
            .unwrap_or(self.clone().secret_manager);
        self.credential_configurations = provisioned_config
            .credential_configurations
            .map(|configs| {
                configs
                    .into_iter()
                    .map(|config| config.into())
                    .collect::<Vec<CredentialConfiguration>>()
            })
            .unwrap_or(self.credential_configurations.clone());
        self.signing_algorithms_supported = provisioned_config
            .signing_algorithms_supported
            .map(|map| map.into_iter().map(|(alg, options)| (alg, options.into())).collect())
            .unwrap_or(self.signing_algorithms_supported.clone());
        self.display = provisioned_config
            .display
            .map(|vec_display| vec_display.into_iter().map(|display| display.into()).collect())
            .unwrap_or(self.display.clone());
        self.event_publishers = provisioned_config
            .event_publishers
            .map(|publishers| publishers.into())
            .or(self.event_publishers.clone());
        self.vp_formats = provisioned_config
            .vp_formats
            .map(|map| {
                map.into_iter()
                    .map(|(claim_format_designation, options)| (claim_format_designation, options.into()))
                    .collect()
            })
            .unwrap_or(self.vp_formats.clone());
        self.clone()
    }

    /// Validates whether the configuration is suitable for production (enforce restrictions).
    pub fn validate(&self) -> Result<(), SharedError> {
        if self.secret_manager.stronghold_password.is_none()
            || self.secret_manager.stronghold_password.as_ref().unwrap().is_empty()
        {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Stronghold password missing".to_string(),
            ));
        }

        if std::env::var("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD")
            .ok()
            .is_none()
        {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Stronghold password must be provided as environment variable".to_string(),
            ));
        }

        // Password policy
        // TODO: refine
        if self.secret_manager.stronghold_password.as_ref().unwrap().len() < 12 {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Stronghold password must be at least 12 characters long".to_string(),
            ));
        }

        if self.event_store.type_ == EventStoreType::InMemory {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Events persisted in-memory would be lost on restart".to_string(),
            ));
        }

        if self.event_store.connection_string.is_none() {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Event store connection string must be provided".to_string(),
            ));
        }

        if self.url.is_none() {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "UniCore URL must be provided".to_string(),
            ));
        }

        Ok(())
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

/// Serializes the passed `String` into the value `"<REDACTED>"` to prevent leaking secrets.
pub(crate) fn redact<S>(str: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    str.as_ref().map(|_| "<REDACTED>".to_string()).serialize(serializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    use provisioned::EventStoreConfig;
    use serial_test::serial;

    #[test]
    fn all_supported_did_methods_can_be_converted_into_subject_syntax_type() {
        for variant in SupportedDidMethod::VARIANTS {
            let _subject_syntax_type: SubjectSyntaxType = (*variant).into();
        }
    }

    #[test]
    fn test_redact_custom_serializer_overwrites_value() {
        let value = EventStoreConfig {
            type_: Some(EventStoreType::Postgres),
            connection_string: Some("postgres://localhost:5432".to_string()),
        };

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, json!({"type": "postgres", "connection_string": "<REDACTED>"}));
    }

    #[test]
    fn test_redact_custom_serializer_ignores_none() {
        let value = EventStoreConfig {
            type_: Some(EventStoreType::InMemory),
            connection_string: None,
        };

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, json!({"type": "in_memory"}));
    }

    #[test]
    fn provisioned_config_successfully_merged_into_default_config() {
        let mut default_config = ApplicationConfiguration::default();

        let provisioned_config = ProvisionedApplicationConfiguration {
            log_format: Some(LogFormat::Text),
            event_store: Some(EventStoreConfig {
                type_: Some(EventStoreType::InMemory),
                connection_string: None,
            }),
            cors_enabled: Some(true),
            domain_linkage_enabled: Some(true),
            secret_manager: Some(provisioned::SecretManagerConfig {
                stronghold_password: Some("sup3rSecr3t".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged_config = default_config.merge(provisioned_config);

        assert_eq!(
            serde_json::to_value(&merged_config).unwrap(),
            json!({
                "log_format": "text",
                "event_store": {
                    "type": "in_memory"
                },
                "cors_enabled": true,
                "domain_linkage_enabled": true,
                "secret_manager": {
                    "stronghold_path": "./stronghold.dat",
                    "stronghold_password": "<REDACTED>",
                    "issuer_eddsa_key_id": "ed25519-0",
                    "issuer_es256_key_id": "es256-0"
                }
            })
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_without_stronghold_password_fails() {
        temp_env::with_var("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some(""), || {
            let config = ApplicationConfiguration::new();

            assert_eq!(
                config.unwrap_err().to_string(),
                "Configuration is not suitable for production: Stronghold password missing"
            );
        });
    }

    #[test]
    #[serial]
    fn test_validate_production_config_when_disrespecting_password_policy_fails() {
        temp_env::with_var(
            "UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD",
            Some("too_short"),
            || {
                let config = ApplicationConfiguration::new();

                assert_eq!(
                    config.unwrap_err().to_string(),
                    "Configuration is not suitable for production: Stronghold password must be at least 12 characters long"
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_disallows_in_memory_persistence() {
        temp_env::with_var("UNICORE__EVENT_STORE__TYPE", Some("in_memory"), || {
            let config = ApplicationConfiguration::new();

            assert_eq!(
                config.unwrap_err().to_string(),
                "Configuration is not suitable for production: Events persisted in-memory would be lost on restart"
            );
        });
    }

    #[test]
    #[serial]
    fn test_validate_production_config_requires_database_connection_string() {
        let config = ApplicationConfiguration::new();

        assert_eq!(
            config.unwrap_err().to_string(),
            "Configuration is not suitable for production: Event store connection string must be provided"
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_requires_explicit_url() {
        temp_env::with_vars(
            [
                ("UNICORE__CONFIG_FILE", Some("./config.yaml")),
                ("UNICORE__EVENT_STORE__CONNECTION_STRING", Some("postgresql://test")),
            ],
            || {
                let config = ApplicationConfiguration::new();

                assert_eq!(
                    config.unwrap_err().to_string(),
                    "Configuration is not suitable for production: UniCore URL must be provided"
                );
            },
        );
    }
}

#[cfg(test)]
pub mod new_application_configuration_tests2 {
    use super::*;
    use config_macro::ConfigImpl;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJson;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJsonParameters;
    use oid4vci::credential_format_profiles::w3c_verifiable_credentials::CredentialSubject;
    use oid4vci::credential_format_profiles::Parameters;

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Clone, Serialize, ConfigImpl)]
    pub struct ApplicationConfiguration {
        #[config_impl(default = "None", development_default = "Some(3033)")]
        pub port: Option<u16>,
        // #[config_impl(default = "LogFormat::Json")]
        // pub log_format: LogFormat,
        // #[config_impl(development_default = "EventStoreConfig {
        //         type_: EventStoreType::InMemory,
        //         connection_string: None
        //     }")]
        // pub event_store: EventStoreConfig,
        // #[config_impl(development_default = r#"Url::parse("http://localhost:3033").unwrap()"#)]
        // pub url: Url,
        // #[config_impl(default = "None")]
        // pub base_path: Option<String>,
        // #[config_impl(default = "false")]
        // pub cors_enabled: bool,
        // #[config_impl(
        //     default = "HashMap::default()",
        //     development_default = "HashMap::from(
        //         [
        //             (
        //                 SupportedDidMethod::Jwk,
        //                 ToggleOptions {
        //                     enabled: true,
        //                     preferred: Some(true)
        //                 }
        //             ),
        //             (
        //                 SupportedDidMethod::Key,
        //                 ToggleOptions {
        //                     enabled: true,
        //                     preferred: None
        //                 }
        //             )
        //         ]
        //     )",
        //     production_default = "HashMap::from(
        //         [
        //             (
        //                 SupportedDidMethod::Jwk,
        //                 ToggleOptions {
        //                     enabled: false,
        //                     preferred: None
        //                 }
        //             ),
        //             (
        //                 SupportedDidMethod::Key,
        //                 ToggleOptions {
        //                     enabled: false,
        //                     preferred: None
        //                 }
        //             ),
        //             (
        //                 SupportedDidMethod::Web,
        //                 ToggleOptions {
        //                     enabled: true,
        //                     preferred: Some(true)
        //                 }
        //             )
        //         ]
        //     )"
        // )]
        // pub did_methods: HashMap<SupportedDidMethod, ToggleOptions>,
        // #[config_impl(default = "2000")]
        // pub external_server_response_timeout_ms: u64,
        // #[config_impl(default = "false", production_default = "true")]
        // pub domain_linkage_enabled: bool,
        // #[config_impl(default = "false")]
        // pub credential_offer_by_value_enabled: bool,
        // // FIXME: implement this very carefully
        // // secret_manager: SecretManagerConfig
        // #[config_impl(
        //     default = "Vec::default()",
        //     development_default = r#"vec![
        //         CredentialConfiguration {
        //             credential_configuration_id: "001".to_string(),
        //             credential_format_with_parameters: CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
        //                 parameters: JwtVcJsonParameters {
        //                     credential_definition: CredentialDefinition {
        //                         type_: vec!["VerifiableCredential".to_string()],
        //                         credential_subject: CredentialSubject::default(),
        //                     },
        //                     order: None,
        //                 },
        //             }),
        //             display: vec![serde_json::to_value(Display {
        //                 name: "Verifiable Credential".to_string(),
        //                 locale: Some("en".to_string()),
        //                 logo: Some(Logo {
        //                     uri: Some(Url::parse("https://www.impierce.com/external/impierce-logo.png").unwrap()),
        //                     alt_text: Some("Impierce Logo".to_string()),
        //                 }),
        //             })
        //             .unwrap()]
        //         }
        //     ]"#
        // )]
        // pub credential_configurations: Vec<CredentialConfiguration>,
        // #[config_impl(default = "HashMap::default()")]
        // pub signing_algorithms_supported: HashMap<Algorithm, ToggleOptions>,
        // #[config_impl(
        //     default = "Vec::default()",
        //     development_default = r#"vec![
        //         Display {
        //             name: "UniCore".to_string(),
        //             locale: Some("en".to_string()),
        //             logo: Some(Logo {
        //                 uri: Some(Url::parse("https://www.impierce.com/external/impierce-logo.png").unwrap()),
        //                 alt_text: Some("Impierce Logo".to_string()),
        //             }),
        //         }
        //     ]"#
        // )]
        // pub display: Vec<Display>,
        // #[config_impl(default = "EventPublishers::default()")]
        // pub event_publishers: EventPublishers,
        // #[config_impl(default = "HashMap::from(
        //         [
        //             (
        //                 ClaimFormatDesignation::JwtVcJson,
        //                 ToggleOptions {
        //                     enabled: true,
        //                     preferred: Some(true)
        //                 }
        //             ),
        //             (
        //                 ClaimFormatDesignation::JwtVpJson,
        //                 ToggleOptions {
        //                     enabled: true,
        //                     preferred: None
        //                 }
        //             )
        //         ]
        //     )")]
        // pub vp_formats: HashMap<ClaimFormatDesignation, ToggleOptions>,
    }

    impl ApplicationConfiguration {
        pub fn load() -> Result<Self, SharedError> {
            let application_profile = ApplicationProfile::load();

            let mut builder = config::Config::builder();

            // // Load the appropriate .env file
            // if cfg!(feature = "test_utils") {
            //     dotenvy::from_filename("../.env.test").ok();
            // }

            let config_file_path_str = std::env::var("UNICORE__CONFIG_FILE").unwrap_or_else(|_| {
                if cfg!(feature = "test_utils") {
                    "../agent_shared/tests/test.config.yaml".to_string()
                } else {
                    "./config.yaml".to_string()
                }
            });

            let config_file_path = std::path::Path::new(&config_file_path_str);

            if config_file_path.exists() {
                builder = builder.add_source(config::File::with_name(&config_file_path_str));
                println!("Loaded config file: `{}`", config_file_path.display());
                info!("Loaded config file: `{}`", config_file_path.display());
            } else {
                println!("Config file not found: `{}`", config_file_path.display());
                warn!("Config file not found: `{}`", config_file_path.display());
            }

            builder = builder.add_source(config::Environment::with_prefix("UNICORE").separator("__"));

            let provisioned_config = builder.build().unwrap();

            Self::load2(provisioned_config, application_profile)
        }
    }

    #[test]
    fn test_example_4321() {
        let config = config().clone();
        println!("{}", serde_json::to_string_pretty(&config).unwrap());

        let provisioned_config = config.to_provisioned_config();

        println!("{}", serde_json::to_string_pretty(&provisioned_config).unwrap());
    }

    /// The `ConfigImpl` trait defines a contract for configuration types that can be used with the `Config` wrapper.
    /// It provides methods for loading, creating, and managing configuration values, including defaults and provisioned values.
    pub trait ConfigImpl: std::ops::Deref
    where
        Self: Sized,
        Self::Target: serde::de::DeserializeOwned + Serialize,
    {
        /// The name of the configuration field, used for serialization and deserialization.
        const NAME: &str;

        /// Creates an instance of the configuration type from its inner value.
        fn from_inner(inner: Self::Target) -> Self;

        /// Creates a provisioned `Config` instance from the inner value.
        /// Marks the configuration as provisioned.
        fn from_provisioned(inner: Self::Target) -> Config<Self> {
            Config {
                provisioned: true,
                inner: Self::from_inner(inner),
            }
        }

        /// Loads the provisioned configuration from the provided configuration source.
        /// Returns `Ok(Some(Config<Self>))` if the configuration is found and valid, or `Ok(None)` if not found.
        fn load_provisioned_config(provisioned_config: &config::Config) -> Result<Option<Config<Self>>, SharedError> {
            if let Ok(value) = provisioned_config.get::<config::Value>(Self::NAME) {
                println!("Found provisioned value for {}: {:?}", Self::NAME, value);
                let inner = value
                    .try_deserialize::<Self::Target>()
                    // If the value is not found, return an error.
                    .map_err(|e| SharedError::ConfigurationNotSuitableForProduction(e.to_string()))?;

                Ok(Some(Self::from_provisioned(inner)))
            } else {
                // If the value is not found, return None.
                // This is not an error, as the configuration may not be required or may have a default value.
                Ok(None)
            }
        }

        /// Provides the default value for the configuration in a development environment.
        /// Returns `None` if no default is defined.
        fn development_default() -> Option<Self::Target> {
            None
        }

        /// Provides the default value for the configuration in a production environment.
        /// Returns `None` if no default is defined.
        fn production_default() -> Option<Self::Target> {
            None
        }

        fn default() -> Option<Self::Target> {
            None
        }

        /// Loads the configuration by first attempting to load a provisioned value.
        /// If no provisioned value is found, it falls back to the default value based on the application profile.
        /// Returns an error if neither a provisioned value nor a default value is available.
        fn load(
            provisioned_config: &config::Config,
            application_profile: &ApplicationProfile,
        ) -> Result<Config<Self>, SharedError> {
            // Load the provisioned value if it exists.
            let provisioned_value: Option<Config<Self>> = Self::load_provisioned_config(provisioned_config)?;

            provisioned_value
                .or_else(|| {
                    // If no provisioned value is found, use the default value.
                    let inner = match application_profile {
                        ApplicationProfile::Development => Self::development_default(),
                        ApplicationProfile::Production => Self::production_default(),
                    }
                    .or_else(|| Self::default());

                    inner.map(|inner| Config {
                        provisioned: false,
                        inner: Self::from_inner(inner),
                    })
                })
                .ok_or_else(|| {
                    SharedError::ConfigurationNotSuitableForProduction(format!(
                        "No default value found for the configuration: {}",
                        Self::NAME
                    ))
                })
        }
    }

    pub static PROVISIONING_METADATA: Lazy<RwLock<HashMap<String, bool>>> = Lazy::new(|| RwLock::new(HashMap::new()));

    pub static CONFIG: Lazy<RwLock<ApplicationConfiguration>> =
        Lazy::new(|| RwLock::new(ApplicationConfiguration::load().unwrap()));

    pub fn config() -> RwLockReadGuard<'static, ApplicationConfiguration> {
        CONFIG.read().unwrap()
    }

    #[skip_serializing_none]
    #[derive(Debug, Clone, Serialize, Deserialize, derive_more::Deref)]
    pub struct Config<T: ConfigImpl>
    where
        T::Target: serde::de::DeserializeOwned + Serialize,
    {
        #[serde(skip)]
        pub provisioned: bool,
        #[deref]
        #[serde(flatten)]
        pub inner: T,
    }

    impl<T: ConfigImpl> Config<T>
    where
        T::Target: serde::de::DeserializeOwned + Serialize,
    {
        pub fn get(&self) -> &T::Target {
            &*self.inner
        }
    }
}
