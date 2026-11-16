//! STACKIT KMS key provider: keys are generated in, and sign inside, a key ring of the customer's STACKIT project.
//!
//! `UniCore` acts as a STACKIT service account. In a deployment it signs in with workload identity federation, so no
//! secret is kept; see `docs/adr/0011-key-providers-sign-in-with-workload-identity.md`.
//!
//! A key is named by `<key id>/<version>`, which pins one version of a STACKIT key, by its key id alone, which means
//! its newest usable version, or by the label it was generated with, which is its display name. STACKIT doesn't keep
//! display names unique, so a label is only given when no key that isn't deleted has it yet. Deleting a key
//! schedules its deletion: STACKIT keeps it for 30 days, during which it counts as deleted here and can still be
//! restored in STACKIT.

mod token;

pub use token::{AccessTokens, ServiceAccountKey, TokenExchange, DEFAULT_FEDERATED_TOKEN_FILE, TOKEN_URL};

use agent_kms::{
    adapter::{
        ConnectionError, InvalidLabel, KeyProviderAdapter, KeyProviderFactory, ProviderKey, ProviderKeyState,
        ProviderKeys,
    },
    key_provider::settings::{KeyProviderSettings, StackitKmsSettings},
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use identity_kit::{
    jose::{ecdsa_signature_from_der, Jwk, JwsAlgorithm, KeyType},
    keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError},
};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

/// Where the STACKIT KMS API is.
pub const API_URL: &str = "https://kms.api.stackit.cloud";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a new key may take to get a version it can sign with, and how often that is checked meanwhile.
const KEY_READY_TIMEOUT: Duration = Duration::from_secs(30);
const KEY_READY_POLL: Duration = Duration::from_millis(500);

/// STACKIT's name for the one algorithm the provider signs with, and the purpose such keys need.
const ES256_ALGORITHM: &str = "ecdsa_p256_sha256";
const SIGNING_PURPOSE: &str = "asymmetric_sign_verify";
/// The protection new keys get. STACKIT also offers `hsm`.
const PROTECTION: &str = "software";
/// The longest display name STACKIT accepts.
const MAX_LABEL_LENGTH: usize = 64;

/// An error reported by STACKIT, or a request it would refuse.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct StackitError(pub String);

/// A key provider in a key ring of a STACKIT project.
pub struct StackitKmsProvider {
    http: reqwest::Client,
    /// The key ring's URL, without a trailing slash.
    key_ring_url: String,
    tokens: Arc<dyn AccessTokens>,
}

impl StackitKmsProvider {
    /// A provider for the key ring in `settings`, reached at `api_url` and signed in to with `tokens`.
    #[must_use]
    pub fn new(
        http: reqwest::Client,
        api_url: &Url,
        settings: &StackitKmsSettings,
        tokens: Arc<dyn AccessTokens>,
    ) -> Self {
        let key_ring_url = format!(
            "{}/v1/projects/{}/regions/{}/keyrings/{}",
            api_url.as_str().trim_end_matches('/'),
            settings.project_id,
            settings.region,
            settings.key_ring_id
        );
        Self {
            http,
            key_ring_url,
            tokens,
        }
    }

    /// Sends a request to `path` within the key ring. Returns `None` when STACKIT answers that it doesn't exist.
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<T>, StackitError> {
        let url = if path.is_empty() {
            self.key_ring_url.clone()
        } else {
            format!("{}/{path}", self.key_ring_url)
        };
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(self.tokens.access_token().await?);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| StackitError(format!("STACKIT KMS can't be reached: {error}")))?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status == StatusCode::UNAUTHORIZED {
            self.tokens.forget().await;
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            struct HttpError {
                message: String,
            }
            let body = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<HttpError>(&body).map_or(body, |error| error.message);
            return Err(StackitError(format!("STACKIT KMS answered {status}: {message}")));
        }

        // Answers without content, such as a scheduled deletion, read as `null`.
        let body = response
            .bytes()
            .await
            .map_err(|error| StackitError(format!("STACKIT KMS's answer can't be read: {error}")))?;
        let value: Value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body)
                .map_err(|error| StackitError(format!("STACKIT KMS answered unexpectedly: {error}")))?
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| StackitError(format!("STACKIT KMS answered unexpectedly: {error}")))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<Option<T>, StackitError> {
        self.request(Method::GET, path, None).await
    }

    async fn keys(&self) -> Result<Vec<StackitKey>, StackitError> {
        #[derive(Deserialize)]
        struct KeyList {
            keys: Vec<StackitKey>,
        }
        Ok(self
            .get::<KeyList>("keys")
            .await?
            .ok_or_else(|| StackitError("The key ring doesn't exist".to_string()))?
            .keys)
    }

    /// The key `key` names, unless there is none or it is deleted.
    async fn find(&self, key: &KeyId) -> Result<Option<StackitKey>, KeyStoreError> {
        let found = match KeyName::of(key) {
            KeyName::Version(id, _) | KeyName::Key(id) => self.get::<StackitKey>(&format!("keys/{id}")).await,
            KeyName::Label(label) => {
                let mut named = self
                    .keys()
                    .await
                    .map_err(KeyStoreError::backend)?
                    .into_iter()
                    .filter(|candidate| !candidate.is_deleted() && candidate.display_name == label);
                match (named.next(), named.next()) {
                    (Some(_), Some(_)) => {
                        return Err(KeyStoreError::invalid_key(
                            key.clone(),
                            "several keys in the key ring have this label",
                        ))
                    }
                    (found, _) => Ok(found),
                }
            }
        };
        Ok(found
            .map_err(KeyStoreError::backend)?
            .filter(|found| !found.is_deleted()))
    }

    /// The key `key` names and the version of it to sign with, after checking that it signs with ES256.
    async fn signing_version(&self, key: &KeyId) -> Result<(StackitKey, Version), KeyStoreError> {
        let found = self
            .find(key)
            .await?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
        if !found.signs_es256() {
            return Err(KeyStoreError::invalid_key(
                key.clone(),
                "the key doesn't sign with ES256",
            ));
        }

        let version = match KeyName::of(key) {
            KeyName::Version(id, number) => self
                .get::<Version>(&format!("keys/{id}/versions/{number}"))
                .await
                .map_err(KeyStoreError::backend)?
                .filter(|version| version.state != "destroyed")
                .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?,
            _ => self
                .usable_version(found.id)
                .await
                .map_err(KeyStoreError::backend)?
                .ok_or_else(|| {
                    KeyStoreError::backend(StackitError(format!("The key `{key}` has no version it can sign with")))
                })?,
        };
        Ok((found, version))
    }

    /// The newest version of the key `id` that can sign, if any.
    async fn usable_version(&self, id: Uuid) -> Result<Option<Version>, StackitError> {
        #[derive(Deserialize)]
        struct VersionList {
            versions: Vec<Version>,
        }
        Ok(self
            .get::<VersionList>(&format!("keys/{id}/versions"))
            .await?
            .map(|list| list.versions)
            .unwrap_or_default()
            .into_iter()
            .filter(Version::is_usable)
            .max_by_key(|version| version.number))
    }

    /// Creates a key and waits until it has a version it can sign with. Returns the key's id in the store, which is
    /// its label when it has one, and the id that pins the version.
    async fn create_key(&self, spec: KeySpec) -> Result<(KeyId, KeyId), KeyStoreError> {
        spec.validate()?;
        if spec.key_type != KeyType::P256 {
            return Err(KeyStoreError::UnsupportedKeyType(spec.key_type));
        }
        if spec.alg != JwsAlgorithm::ES256 {
            return Err(KeyStoreError::UnsupportedAlgorithm(spec.alg));
        }
        if let Some(label) = &spec.label {
            check_label(label).map_err(KeyStoreError::backend)?;
            if self.find(&KeyId::new(label.as_str())).await?.is_some() {
                return Err(KeyStoreError::AlreadyExists(KeyId::new(label.as_str())));
            }
        }

        let display_name = spec
            .label
            .clone()
            .unwrap_or_else(|| format!("unicore-{}", Uuid::new_v4()));
        let created = self
            .request::<StackitKey>(
                Method::POST,
                "keys",
                Some(json!({
                    "displayName": display_name,
                    "algorithm": ES256_ALGORITHM,
                    "purpose": SIGNING_PURPOSE,
                    "protection": PROTECTION,
                })),
            )
            .await
            .map_err(KeyStoreError::backend)?
            .ok_or_else(|| KeyStoreError::backend(StackitError("The key ring doesn't exist".to_string())))?;

        let version = tokio::time::timeout(KEY_READY_TIMEOUT, async {
            loop {
                if let Some(version) = self.usable_version(created.id).await? {
                    return Ok::<_, StackitError>(version);
                }
                tokio::time::sleep(KEY_READY_POLL).await;
            }
        })
        .await
        .map_err(|_| {
            KeyStoreError::backend(StackitError(format!(
                "The key `{}` was created, but had no version to sign with after {} seconds",
                created.id,
                KEY_READY_TIMEOUT.as_secs()
            )))
        })?
        .map_err(KeyStoreError::backend)?;

        let pinned = KeyId::new(format!("{}/{}", created.id, version.number));
        let name = spec.label.map_or_else(|| pinned.clone(), KeyId::new);
        Ok((name, pinned))
    }
}

#[async_trait]
impl KeyStore for StackitKmsProvider {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new([JwsAlgorithm::ES256])
            .with_generate()
            .with_delete()
            .remote()
    }

    async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
        let (_, version) = self.signing_version(key).await?;
        let pem = version
            .public_key
            .ok_or_else(|| KeyStoreError::invalid_key(key.clone(), "STACKIT KMS returned no public key"))?;
        let der = spki_der_from_pem(&pem)
            .ok_or_else(|| KeyStoreError::invalid_key(key.clone(), "the public key from STACKIT KMS isn't PEM"))?;
        let jwk = Jwk::from_spki_der(KeyType::P256, &der).map_err(|error| KeyStoreError::InvalidKey {
            key: key.clone(),
            reason: "the public key from STACKIT KMS does not decode".to_string(),
            source: Some(Box::new(error)),
        })?;
        Ok(jwk.with_alg(JwsAlgorithm::ES256.as_str()).with_kid(key.as_str()))
    }

    async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        #[derive(Deserialize)]
        struct SignedData {
            signature: String,
        }
        let (found, version) = self.signing_version(key).await?;
        if alg != JwsAlgorithm::ES256 {
            return Err(KeyStoreError::AlgorithmMismatch {
                key: key.clone(),
                key_alg: JwsAlgorithm::ES256,
                requested: alg,
            });
        }

        // STACKIT hashes the whole message itself.
        let signed = self
            .request::<SignedData>(
                Method::POST,
                &format!("keys/{}/versions/{}/sign", found.id, version.number),
                Some(json!({ "data": STANDARD.encode(data) })),
            )
            .await
            .map_err(KeyStoreError::backend)?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
        let der = STANDARD.decode(signed.signature).map_err(|error| {
            KeyStoreError::backend(StackitError(format!("STACKIT KMS's signature isn't base64: {error}")))
        })?;

        // STACKIT returns ECDSA signatures DER-encoded; JWS wants raw `r || s`.
        ecdsa_signature_from_der(alg, &der).map_err(KeyStoreError::backend)
    }

    async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        let Some(found) = self.find(key).await? else {
            return Ok(false);
        };
        match KeyName::of(key) {
            KeyName::Version(_, number) => Ok(self
                .get::<Version>(&format!("keys/{}/versions/{number}", found.id))
                .await
                .map_err(KeyStoreError::backend)?
                .is_some_and(|version| version.state != "destroyed")),
            _ => Ok(true),
        }
    }

    async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        Ok(self.create_key(spec).await?.0)
    }

    async fn delete(&self, key: &KeyId) -> Result<(), KeyStoreError> {
        let found = self
            .find(key)
            .await?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
        self.request::<Value>(Method::DELETE, &format!("keys/{}", found.id), None)
            .await
            .map_err(KeyStoreError::backend)?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
        Ok(())
    }
}

#[async_trait]
impl KeyProviderAdapter for StackitKmsProvider {
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        match self.get::<Value>("").await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(ConnectionError(
                "The key ring doesn't exist in this STACKIT project".to_string(),
            )),
            Err(error) => Err(ConnectionError(error.0)),
        }
    }

    /// Lists the keys in the key ring that sign with ES256. Keys for other purposes can't sign credentials, so they
    /// are left out.
    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
        let keys = self.keys().await.map_err(|error| ConnectionError(error.0))?;
        Ok(ProviderKeys {
            keys: keys
                .into_iter()
                .filter(StackitKey::signs_es256)
                .map(|key| ProviderKey {
                    id: key.id.to_string(),
                    resource_name: None,
                    state: key.provider_state(),
                })
                .collect(),
            hidden_keys: 0,
        })
    }

    fn check_label(&self, label: &str) -> Result<(), InvalidLabel> {
        check_label(label)
    }

    /// Returns the id that pins the key's first version, which names this key for good, whereas a label can be
    /// given to another key once this one is deleted.
    async fn create(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        Ok(self.create_key(spec).await?.1)
    }
}

/// Builds STACKIT KMS providers that sign in with the workload identity token the host keeps in a file.
pub struct StackitKmsFactory {
    http: reqwest::Client,
    api_url: Url,
    token_url: Url,
    federated_token_file: PathBuf,
}

impl StackitKmsFactory {
    /// A factory whose providers read the workload identity token from `federated_token_file`, or from where
    /// STACKIT's own SDKs look for it.
    ///
    /// # Errors
    ///
    /// Fails when no HTTP client can be built, such as when TLS isn't available.
    pub fn new(federated_token_file: Option<PathBuf>) -> Result<Self, StackitError> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| StackitError(format!("No HTTP client for STACKIT can be built: {error}")))?;
        Ok(Self {
            http,
            api_url: Url::parse(API_URL).map_err(|error| StackitError(error.to_string()))?,
            token_url: Url::parse(TOKEN_URL).map_err(|error| StackitError(error.to_string()))?,
            federated_token_file: federated_token_file.unwrap_or_else(|| PathBuf::from(DEFAULT_FEDERATED_TOKEN_FILE)),
        })
    }
}

impl KeyProviderFactory for StackitKmsFactory {
    fn build(&self, settings: Option<&KeyProviderSettings>) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError> {
        let Some(KeyProviderSettings::StackitKms(settings)) = settings else {
            return Err(ConnectionError("A STACKIT KMS provider needs its settings".to_string()));
        };
        let tokens = TokenExchange::workload_identity(
            self.http.clone(),
            self.federated_token_file.clone(),
            settings.service_account_email.clone(),
        )
        .with_token_url(self.token_url.clone());
        Ok(Arc::new(StackitKmsProvider::new(
            self.http.clone(),
            &self.api_url,
            settings,
            Arc::new(tokens),
        )))
    }
}

/// What a [`KeyId`] names.
#[derive(Debug, PartialEq)]
enum KeyName {
    /// One version of a key.
    Version(Uuid, u64),
    /// A key, by its newest usable version.
    Key(Uuid),
    /// The key with this display name.
    Label(String),
}

impl KeyName {
    fn of(key: &KeyId) -> Self {
        let id = key.as_str();
        if let Some((key_id, version)) = id.split_once('/') {
            if let (Ok(key_id), Ok(version)) = (Uuid::parse_str(key_id), version.parse()) {
                return Self::Version(key_id, version);
            }
        } else if let Ok(key_id) = Uuid::parse_str(id) {
            return Self::Key(key_id);
        }
        Self::Label(id.to_string())
    }
}

/// Checks that STACKIT accepts `label` as a display name, and that it can't be taken for a key id.
fn check_label(label: &str) -> Result<(), InvalidLabel> {
    let valid = !label.is_empty()
        && label.len() <= MAX_LABEL_LENGTH
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && Uuid::parse_str(label).is_err();
    if valid {
        Ok(())
    } else {
        Err(InvalidLabel(format!(
            "`{label}` can't be a STACKIT KMS key name: use at most {MAX_LABEL_LENGTH} letters, digits, underscores \
             and hyphens, and not a UUID"
        )))
    }
}

/// The DER bytes of a PEM-encoded public key.
fn spki_der_from_pem(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();
    STANDARD.decode(body).ok()
}

/// A key in a STACKIT key ring.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackitKey {
    id: Uuid,
    display_name: String,
    state: String,
    purpose: String,
    algorithm: String,
}

impl StackitKey {
    fn is_deleted(&self) -> bool {
        self.state == "deleted"
    }

    fn signs_es256(&self) -> bool {
        self.purpose == SIGNING_PURPOSE && self.algorithm == ES256_ALGORITHM
    }

    fn provider_state(&self) -> ProviderKeyState {
        match self.state.as_str() {
            "active" => ProviderKeyState::Enabled,
            "deleted" => ProviderKeyState::PendingDeletion,
            _ => ProviderKeyState::Unavailable,
        }
    }
}

/// A version of a STACKIT key.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    number: u64,
    state: String,
    #[serde(default)]
    disabled: bool,
    public_key: Option<String>,
}

impl Version {
    fn is_usable(&self) -> bool {
        self.state == "active" && !self.disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "3f2c1a6e-8b7d-4c5e-9a10-2b3c4d5e6f70";

    #[test]
    fn names_a_version_of_a_key() {
        assert_eq!(
            KeyName::of(&KeyId::new(format!("{KEY}/2"))),
            KeyName::Version(Uuid::parse_str(KEY).unwrap(), 2)
        );
    }

    #[test]
    fn names_a_key_by_its_id() {
        assert_eq!(
            KeyName::of(&KeyId::new(KEY)),
            KeyName::Key(Uuid::parse_str(KEY).unwrap())
        );
    }

    #[test]
    fn names_a_key_by_its_label() {
        assert_eq!(KeyName::of(&KeyId::new("issuer")), KeyName::Label("issuer".to_string()));
    }

    #[test]
    fn accepts_a_label_of_letters_digits_underscores_and_hyphens() {
        assert!(check_label("issuer_2-eu").is_ok());
    }

    #[test]
    fn refuses_a_label_with_a_slash() {
        assert!(check_label("team/issuer").is_err());
    }

    #[test]
    fn refuses_a_label_longer_than_stackit_accepts() {
        assert!(check_label(&"a".repeat(MAX_LABEL_LENGTH + 1)).is_err());
    }

    #[test]
    fn refuses_a_label_that_reads_as_a_key_id() {
        assert!(check_label(KEY).is_err());
    }

    #[test]
    fn reads_the_der_of_a_pem_public_key() {
        let pem = "-----BEGIN PUBLIC KEY-----\nAAEC\nAwQ=\n-----END PUBLIC KEY-----\n";

        assert_eq!(spki_der_from_pem(pem), Some(vec![0, 1, 2, 3, 4]));
    }
}
