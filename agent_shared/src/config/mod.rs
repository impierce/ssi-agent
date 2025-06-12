mod provisioned;

use agent_macros::Config;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use identity_iota::storage::KeyId;
use jsonwebtoken::Algorithm;
use oid4vc_core::SubjectSyntaxType;
use oid4vci::credential_format_profiles::{CredentialFormats, WithParameters};
use oid4vp::ClaimFormatDesignation;
use once_cell::sync::Lazy;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_with::{skip_serializing_none, SerializeDisplay};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{RwLock, RwLockReadGuard},
};
use strum::VariantArray;
use url::Url;

use crate::{error::SharedError, profile::ApplicationProfile};
// Re-export
pub use provisioned::load_provisioned_config;

static STRONGHOLD_PATH: &str = "./stronghold.dat";

// TODO: Once we have a proper state implementation for `agent_secret_manager` we can make use of randomly generated Key
// IDs. For now we need to make use of these static variables.
static ED25519_KEY_ID: &str = "ed25519-0";
static ES256_KEY_ID: &str = "es256-0";

// Static configuration instance
pub static CONFIG: Lazy<RwLock<ApplicationConfiguration>> = Lazy::new(|| {
    let application_configuration =
        ApplicationConfiguration::load(load_provisioned_config().unwrap(), ApplicationProfile::load())
            // Fail fast when the configuration is not suitable for the current application profile.
            .unwrap_or_else(|e| panic!("{e}"));

    #[cfg(not(feature = "test_utils"))]
    {
        use tracing::{debug, info};
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

        let tracing_subscriber = tracing_subscriber::registry()
            // Set the default logging level to `info`, equivalent to `RUST_LOG=info`
            .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()));

        match application_configuration.log_format {
            LogFormat::Json => tracing_subscriber.with(tracing_subscriber::fmt::layer().json()).init(),
            LogFormat::Text => tracing_subscriber.with(tracing_subscriber::fmt::layer()).init(),
        }

        info!("Configuration loaded successfully");
        debug!("{:#?}", application_configuration);
    }

    RwLock::new(application_configuration)
});

fn convert_url(url: Url, with_trailing_slash: bool) -> Url {
    let mut new_url = url.clone();
    let mut new_path = new_url.path().trim_matches('/').to_string();
    if with_trailing_slash {
        new_path.push('/');
    }
    new_url.set_path(&new_path);
    new_url
}

/// Converts a URL into a directory URL by adding a trailing slash if it doesn't already have one.
fn into_directory(url: Url) -> Url {
    convert_url(url, true)
}

/// Converts a URL into a resource URL by removing the trailing slash if it has one.
fn into_resource(url: Url) -> Url {
    convert_url(url, false)
}

// Accessor for the configuration
pub fn config() -> RwLockReadGuard<'static, ApplicationConfiguration> {
    CONFIG.read().unwrap()
}

pub fn config_mut() -> std::sync::RwLockWriteGuard<'static, ApplicationConfiguration> {
    CONFIG.write().unwrap()
}

#[skip_serializing_none]
#[derive(Debug, Deserialize, Clone, Serialize, Config)]
pub struct ApplicationConfiguration {
    #[config(default)]
    pub log_format: LogFormat,
    #[config(development_default = "EventStoreConfig {
            type_: EventStoreType::InMemory,
            connection_string: None
        }")]
    pub event_store: EventStoreConfig,
    #[config(
        development_default = r#"{
            Url::parse(&format!("http://localhost:3033")).unwrap()
        }"#,
        transform_with = "into_directory"
    )]
    pub application_url: Url,
    #[config(
        default = "Self::fn_application_url(provisioned_config, application_profile).unwrap()",
        transform_with = "into_directory"
    )]
    pub public_url: Url,
    #[config(
        default = r#"{
            let public_url = Self::fn_public_url(provisioned_config, application_profile).unwrap();

            provisioned_config.get::<Url>("token_endpoint").unwrap_or_else(|_| {
                public_url.join("auth/token").unwrap()
            })
        }"#,
        transform_with = "into_resource"
    )]
    pub token_endpoint: Url,
    #[config(
        default = r#"{
            let public_url = Self::fn_public_url(provisioned_config, application_profile).unwrap();

            provisioned_config.get::<Url>("credential_endpoint").unwrap_or_else(|_| {
                public_url.join("openid4vci/credential").unwrap()
            })
        }"#,
        transform_with = "into_resource"
    )]
    pub credential_endpoint: Url,
    #[config(
        default = r#"{
            let public_url = Self::fn_public_url(provisioned_config, application_profile).unwrap();

            provisioned_config.get::<Url>("credential_offer_uri").unwrap_or_else(|_| {
                public_url.join("openid4vci/credential-offer/").unwrap()
            })
        }"#,
        transform_with = "into_resource"
    )]
    pub credential_offer_uri: Url,
    #[config(
        default = r#"{
            let public_url = Self::fn_public_url(provisioned_config, application_profile).unwrap();

            provisioned_config.get::<Url>("request_uri").unwrap_or_else(|_| {
                public_url.join("request/").unwrap()
            })
        }"#,
        transform_with = "into_resource"
    )]
    pub request_uri: Url,
    #[config(
        default = r#"{
            let public_url = Self::fn_public_url(provisioned_config, application_profile).unwrap();

            provisioned_config.get::<Url>("redirect_uri").unwrap_or_else(|_| {
                public_url.join("redirect").unwrap()
            })
        }"#,
        transform_with = "into_resource"
    )]
    pub redirect_uri: Url,
    #[config(default)]
    pub cors_enabled: bool,
    #[config(
        default,
        development_default = "HashMap::from(
            [
                (
                    SupportedDidMethod::Jwk,
                    ToggleOptions {
                        enabled: true,
                        preferred: Some(true)
                    }
                ),
                (
                    SupportedDidMethod::Key,
                    ToggleOptions {
                        enabled: true,
                        preferred: None
                    }
                )
            ]
        )",
        production_default = "HashMap::from(
            [
                (
                    SupportedDidMethod::Jwk,
                    ToggleOptions {
                        enabled: false,
                        preferred: None
                    }
                ),
                (
                    SupportedDidMethod::Key,
                    ToggleOptions {
                        enabled: false,
                        preferred: None
                    }
                ),
                (
                    SupportedDidMethod::Web,
                    ToggleOptions {
                        enabled: true,
                        preferred: Some(true)
                    }
                )
            ]
        )"
    )]
    pub did_methods: HashMap<SupportedDidMethod, ToggleOptions>,
    #[config(default = "1000")]
    pub external_server_response_timeout_ms: u64,
    #[config(default, production_default = "true")]
    pub domain_linkage_enabled: bool,
    #[config(default)]
    pub credential_offer_by_value_enabled: bool,
    #[config(development_default = "SecretManagerConfig::development_default()")]
    pub secret_manager: SecretManagerConfig,
    #[config(
        default,
        development_default = r#"
        Some(Box::new(Path::new("./config/credential_configurations.json").to_owned()))
    "#
    )]
    pub credential_configuration_file: Option<Box<PathBuf>>,
    #[config(default = "
        HashMap::from(
            [
                (
                    Algorithm::EdDSA,
                    ToggleOptions {
                        enabled: true,
                        preferred: Some(true)
                    }
                ),
                (
                    Algorithm::ES256,
                    ToggleOptions {
                        enabled: true,
                        preferred: None
                    }
                )
            ]
        )")]
    pub signing_algorithms_supported: HashMap<Algorithm, ToggleOptions>,
    #[config(development_default = r#"vec![
            Display {
                name: "UniCore".to_string(),
                locale: Some("en".to_string()),
                logo: Some(Logo {
                    uri: Some(Url::parse("https://www.impierce.com/external/impierce-icon.png").unwrap()),
                    alt_text: Some("Impierce Icon".to_string()),
                }),
            }
        ]"#)]
    pub display: Vec<Display>,
    #[config(default)]
    pub event_publishers: EventPublishers,
    #[config(default = "HashMap::from(
        [
            (
                ClaimFormatDesignation::JwtVcJson,
                ToggleOptions {
                    enabled: true,
                    preferred: Some(true)
                }
            ),
            (
                ClaimFormatDesignation::JwtVpJson,
                ToggleOptions {
                    enabled: true,
                    preferred: None
                }
            )
        ]
    )")]
    pub vp_formats: HashMap<ClaimFormatDesignation, ToggleOptions>,
}

impl ApplicationConfiguration {
    /// Validates whether the configuration is suitable for development (enforce restrictions).
    pub fn validate_development(&self) -> Result<(), SharedError> {
        if self.event_store.type_ == EventStoreType::InMemory {
            for did_method in &[SupportedDidMethod::Iota, SupportedDidMethod::IotaSmr] {
                if self
                    .did_methods
                    .get(did_method)
                    .map(|options| options.enabled)
                    .unwrap_or_default()
                {
                    return Err(SharedError::InvalidConfiguration(format!(
                        "`{did_method}` cannot be enabled when using the `in_memory` event store"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validates whether the configuration is suitable for production (enforce restrictions).
    pub fn validate(&self) -> Result<(), SharedError> {
        if self.secret_manager.stronghold_password.is_empty() {
            return Err(SharedError::ConfigurationNotSuitableForProduction(
                "Stronghold password missing".to_string(),
            ));
        }

        #[cfg(not(feature = "test_utils"))]
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
        if self.secret_manager.stronghold_password.len() < 12 {
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
        if let Some(http) = &mut self.event_publishers.http {
            http.enabled = true;
        }
    }

    pub fn set_event_publisher_http_target_url(&mut self, target_url: String) {
        if let Some(http) = &mut self.event_publishers.http {
            http.target_url = target_url;
        }
    }

    pub fn set_event_publisher_http_target_events(&mut self, events: Events) {
        if let Some(http) = &mut self.event_publishers.http {
            http.events = events;
        }
    }

    pub fn set_secret_manager_config(&mut self, config: SecretManagerConfig) {
        self.secret_manager = config;
    }
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
    #[serde(rename = "type", default)]
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

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SecretManagerConfig {
    #[serde(default = "default_stronghold_path")]
    pub stronghold_path: String,
    #[serde(serialize_with = "redact")]
    pub stronghold_password: String,
    #[serde(default = "default_issuer_eddsa_key_id")]
    pub issuer_eddsa_key_id: KeyId,
    #[serde(default = "default_issuer_es256_key_id")]
    pub issuer_es256_key_id: KeyId,
}

impl SecretManagerConfig {
    fn development_default() -> Self {
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        let stronghold_password = URL_SAFE_NO_PAD.encode(random_bytes)[..24].to_string();

        println!(
            r#"
            #####################################################
            #                                                   #
            #   A new Stronghold password has been generated!   #
            #                                                   #
            #             {stronghold_password}              #
            #                                                   #
            #####################################################
            "#
        );

        Self {
            stronghold_path: default_stronghold_path(),
            stronghold_password,
            issuer_eddsa_key_id: default_issuer_eddsa_key_id(),
            issuer_es256_key_id: default_issuer_es256_key_id(),
        }
    }
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

#[skip_serializing_none]
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
pub(crate) fn redact<S, T>(_str: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let redacted_value = "<REDACTED>";
    let redacted_value = redacted_value.to_string();
    redacted_value.serialize(serializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn all_supported_did_methods_can_be_converted_into_subject_syntax_type() {
        for variant in SupportedDidMethod::VARIANTS {
            let _subject_syntax_type: SubjectSyntaxType = (*variant).into();
        }
    }

    #[test]
    #[serial]
    fn test_redact_custom_serializer_overwrites_value() {
        let value = EventStoreConfig {
            type_: EventStoreType::Postgres,
            connection_string: Some("postgres://localhost:5432".to_string()),
        };

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, json!({"type": "postgres", "connection_string": "<REDACTED>"}));
    }

    #[test]
    #[serial]
    fn test_redact_custom_serializer_ignores_none() {
        let value = EventStoreConfig {
            type_: EventStoreType::InMemory,
            connection_string: None,
        };

        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(json, json!({"type": "in_memory"}));
    }

    #[test]
    #[serial]
    fn provisioned_config_successfully_merged_into_default_config() {
        let provisioned_config = config::Config::builder()
            .add_source(config::File::from_str(
                r#"
                    log_format: "text"
                    event_store:
                        type: "in_memory"
                    cors_enabled: true
                    domain_linkage_enabled: true
                    secret_manager:
                        stronghold_password: "sup3rSecr3t"
                "#,
                config::FileFormat::Yaml,
            ))
            .build()
            .unwrap();

        let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Development).unwrap();

        assert_eq!(
            json!(config),
            json!({
              "log_format": "text",
              "event_store": {
                "type": "in_memory"
              },
              "application_url": "http://localhost:3033/",
              "public_url": "http://localhost:3033/",
              "token_endpoint": "http://localhost:3033/auth/token",
              "credential_endpoint": "http://localhost:3033/openid4vci/credential",
              "credential_offer_uri": "http://localhost:3033/openid4vci/credential-offer",
              "request_uri": "http://localhost:3033/request",
              "redirect_uri": "http://localhost:3033/redirect",
              "cors_enabled": true,
              "did_methods": {
                "did:jwk": {
                  "enabled": true,
                  "preferred": true
                },
                "did:key": {
                  "enabled": true
                }
              },
              "external_server_response_timeout_ms": 1000,
              "domain_linkage_enabled": true,
              "credential_offer_by_value_enabled": false,
              "secret_manager": {
                "stronghold_path": "./stronghold.dat",
                "stronghold_password": "<REDACTED>",
                "issuer_eddsa_key_id": "ed25519-0",
                "issuer_es256_key_id": "es256-0"
              },
              "credential_configuration_file": "./config/credential_configurations.json",
              "signing_algorithms_supported": {
                "EdDSA": {
                  "enabled": true,
                  "preferred": true
                },
                "ES256": {
                  "enabled": true
                }
              },
              "display": [
                {
                  "name": "UniCore",
                  "locale": "en",
                  "logo": {
                    "uri": "https://www.impierce.com/external/impierce-icon.png",
                    "alt_text": "Impierce Icon"
                  }
                }
              ],
              "event_publishers": {},
              "vp_formats": {
                "jwt_vc_json": {
                  "enabled": true,
                  "preferred": true
                },
                "jwt_vp_json": {
                  "enabled": true
                }
              }
            })
        );

        assert_eq!(
            config.get_provisioned_config(),
            json!({
                "log_format": "text",
                "event_store": {
                    "type": "in_memory"
                },
                "cors_enabled": true,
                "domain_linkage_enabled": true,
                "secret_manager": {
                    "stronghold_password": "<REDACTED>"
                }
            })
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_without_stronghold_password_fails() {
        temp_env::with_var("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some(""), || {
            let provisioned_config = config::Config::builder()
                .add_source(config::File::from_str(
                    r#"
                        application_url: "http://localhost"
                        event_store:
                            type: "in_memory"
                        display:
                            - name: "UniCore"
                    "#,
                    config::FileFormat::Yaml,
                ))
                .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                .build()
                .unwrap();

            let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production);

            assert_eq!(
                config.unwrap_err().to_string(),
                "Configuration is not suitable for production: Stronghold password missing"
            );
        });
    }

    #[test]
    #[serial]
    fn test_validate_production_config_when_disrespecting_password_policy_fails() {
        temp_env::with_vars(
            [("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("too_short"))],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::File::from_str(
                        r#"
                            application_url: "http://localhost"
                            event_store:
                                type: "in_memory"
                            display:
                                - name: "UniCore"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();

                let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production);

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
        temp_env::with_vars(
            [
                ("UNICORE__EVENT_STORE__TYPE", Some("in_memory")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
            ],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::File::from_str(
                        r#"
                            application_url: "http://localhost"
                            display:
                                - name: "UniCore"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();

                let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production);

                assert_eq!(
                    config.unwrap_err().to_string(),
                    "Configuration is not suitable for production: Events persisted in-memory would be lost on restart"
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_requires_database_connection_string() {
        temp_env::with_var(
            "UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD",
            Some("unsafe-password"),
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::File::from_str(
                        r#"
                            application_url: "http://localhost"
                            event_store:
                                type: "postgres"
                            display:
                                - name: "UniCore"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();

                let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production);

                assert_eq!(
                    config.unwrap_err().to_string(),
                    "Configuration is not suitable for production: Event store connection string must be provided"
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_validate_production_config_requires_explicit_application_url() {
        temp_env::with_vars(
            [
                ("UNICORE__EVENT_STORE__CONNECTION_STRING", Some("postgresql://:test:")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
                ("UNICORE__APPLICATION_URL", None::<&str>),
            ],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::File::from_str(
                        r#"
                            event_store:
                                type: "postgres"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();

                let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production);

                assert_eq!(
                    config.unwrap_err().to_string(),
                    "Configuration is not suitable for production: `application_url` must be provided"
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_config_file_not_found_only_includes_provisioned_env_vars() {
        temp_env::with_vars(
            [
                ("UNICORE__CONFIG_FILE", Some("./does-not-exist.yaml")),
                ("UNICORE__EVENT_STORE__CONNECTION_STRING", Some("postgresql://:test:")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
                ("UNICORE__APPLICATION_URL", Some("http://localhost")),
            ],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(load_provisioned_config().unwrap())
                    .add_source(config::File::from_str(
                        r#"
                            display:
                                - name: "UniCore"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .build()
                    .unwrap();

                let config =
                    ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production).unwrap();

                assert_eq!(
                    config.get_provisioned_config(),
                    json!({
                      "application_url": "http://localhost/",
                      "event_store": {
                        "connection_string": "<REDACTED>"
                      },
                      "secret_manager": {
                        "stronghold_password": "<REDACTED>"
                      },
                      "display": [
                        {
                          "name": "UniCore"
                        }
                      ]
                    })
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_loads_config_file() {
        temp_env::with_vars(
            [
                ("UNICORE__CONFIG_FILE", Some("../agent_application/example.config.yaml")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
            ],
            || {
                let provisioned_config = load_provisioned_config().unwrap();

                let config =
                    ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production).unwrap();

                let serialized = serde_json::to_value(&config).unwrap();

                assert_eq!(
                    serialized.get("application_url").unwrap(),
                    &json!("https://ssi-agent.example.org/")
                );
            },
        );
    }

    #[test]
    #[serial]
    fn test_env_var_overwrites_config_file() {
        temp_env::with_vars(
            [
                ("UNICORE__LOG_FORMAT", Some("text")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
            ],
            || {
                let provisioned_config = load_provisioned_config().unwrap();

                let config =
                    ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production).unwrap();

                let serialized = serde_json::to_value(&config).unwrap();

                assert_eq!(serialized.get("log_format").unwrap(), &json!("text"));
            },
        );
    }

    #[test]
    #[serial]
    fn test_development_default_config() {
        // In development mode provisioned config variables are not required so we can use the default config.
        let provisioned_config = config::Config::default();

        let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Development).unwrap();

        // Use in-memory event store (no dependency on a database)
        assert_eq!(config.event_store.type_, EventStoreType::InMemory);

        // Stronghold file is in the default directory
        assert_eq!(config.secret_manager.stronghold_path, "./stronghold.dat");

        // A password is set
        assert!(!config.secret_manager.stronghold_password.is_empty());

        // The UniCore Application URL points to localhost
        assert_eq!(config.application_url, Url::parse("http://localhost:3033").unwrap());

        // The public URL points to localhost
        assert_eq!(config.public_url, Url::parse("http://localhost:3033").unwrap());

        // The OpenID4VCI endpoints are set correctly
        assert_eq!(
            config.token_endpoint,
            Url::parse("http://localhost:3033/auth/token").unwrap()
        );
        assert_eq!(
            config.credential_endpoint,
            Url::parse("http://localhost:3033/openid4vci/credential").unwrap()
        );
        assert_eq!(
            config.credential_offer_uri,
            Url::parse("http://localhost:3033/openid4vci/credential-offer").unwrap()
        );

        // The OpenID4VP endpoints are set correctly
        assert_eq!(config.request_uri, Url::parse("http://localhost:3033/request").unwrap());
        assert_eq!(
            config.redirect_uri,
            Url::parse("http://localhost:3033/redirect").unwrap()
        );

        // Enable centrally hosted DID methods
        assert!(config.did_methods.get(&SupportedDidMethod::Jwk).unwrap().enabled);
        assert!(config.did_methods.get(&SupportedDidMethod::Key).unwrap().enabled);

        // Domain linkage is disabled
        assert!(!config.domain_linkage_enabled);

        // Some display information is set
        assert_eq!(config.display.len(), 1);

        // The Credential Configuration file is set to the default path
        assert_eq!(
            config.credential_configuration_file,
            Some(Box::new(
                Path::new("./config/credential_configurations.json").to_path_buf()
            ))
        );
    }

    #[test]
    #[serial]
    fn test_production_default_config() {
        // In production mode we need to set some required environment variables.
        temp_env::with_vars(
            [
                ("UNICORE__EVENT_STORE__CONNECTION_STRING", Some("postgresql://:test:")),
                ("UNICORE__SECRET_MANAGER__STRONGHOLD_PASSWORD", Some("unsafe-password")),
                ("UNICORE__APPLICATION_URL", Some("http://localhost")),
            ],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::File::from_str(
                        r#"
                            display:
                                - name: "UniCore"
                        "#,
                        config::FileFormat::Yaml,
                    ))
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();
                let config =
                    ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Production).unwrap();

                assert!(config.domain_linkage_enabled);

                // Disable DID methods that do not support updates
                assert!(!config.did_methods.get(&SupportedDidMethod::Jwk).unwrap().enabled);
                assert!(!config.did_methods.get(&SupportedDidMethod::Key).unwrap().enabled);
                assert!(config.did_methods.get(&SupportedDidMethod::Web).unwrap().enabled);
            },
        );
    }

    #[test]
    #[serial]
    fn test_public_url_defaults_to_application_url() {
        let provisioned_config = config::Config::builder()
            .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
            .build()
            .unwrap();
        let config = ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Development).unwrap();

        // Assert that the public URL is set to the application URL
        assert_eq!(config.public_url, config.application_url);
    }

    #[test]
    #[serial]
    fn test_oid4vc_endpoint_variables_overwrite_defaults() {
        temp_env::with_vars(
            [
                (
                    "UNICORE__TOKEN_ENDPOINT",
                    Some("https://my-domain.example.org/my/token/endpoint"),
                ),
                (
                    "UNICORE__CREDENTIAL_ENDPOINT",
                    Some("https://my-domain.example.org/my/credential/endpoint"),
                ),
                (
                    "UNICORE__CREDENTIAL_OFFER_URI",
                    Some("https://my-domain.example.org/my/credential/offer/uri"),
                ),
                (
                    "UNICORE__REQUEST_URI",
                    Some("https://my-domain.example.org/my/request/uri"),
                ),
                (
                    "UNICORE__REDIRECT_URI",
                    Some("https://my-domain.example.org/my/redirect/uri"),
                ),
            ],
            || {
                let provisioned_config = config::Config::builder()
                    .add_source(config::Environment::with_prefix("UNICORE").separator("__"))
                    .build()
                    .unwrap();
                let config =
                    ApplicationConfiguration::load(provisioned_config, ApplicationProfile::Development).unwrap();

                // Assert that the OpenID4VCI endpoints are set correctly and do not contain a trailing slash
                assert_eq!(
                    config.token_endpoint,
                    Url::parse("https://my-domain.example.org/my/token/endpoint").unwrap()
                );
                assert_eq!(
                    config.credential_endpoint,
                    Url::parse("https://my-domain.example.org/my/credential/endpoint").unwrap()
                );
                assert_eq!(
                    config.credential_offer_uri,
                    Url::parse("https://my-domain.example.org/my/credential/offer/uri").unwrap()
                );
                assert_eq!(
                    config.request_uri,
                    Url::parse("https://my-domain.example.org/my/request/uri").unwrap()
                );
                assert_eq!(
                    config.redirect_uri,
                    Url::parse("https://my-domain.example.org/my/redirect/uri").unwrap()
                );
            },
        );
    }
}
