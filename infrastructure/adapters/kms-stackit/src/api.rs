//! The STACKIT KMS REST API (v1) as far as the key provider uses it, for the keys of one key ring.

use crate::answer::refusal_message;
use crate::sign_in::{AccessTokens, SignInError};
use agent_kms::key_provider::settings::StackitKmsSettings;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// Where the STACKIT KMS API is. The region is part of each path.
pub(crate) const KMS_URL: &str = "https://kms.api.stackit.cloud";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The algorithm and purpose of the keys the provider signs with: P-256, ES256.
const ES256_ALGORITHM: &str = "ecdsa_p256_sha256";
const SIGNING_PURPOSE: &str = "asymmetric_sign_verify";

/// Why STACKIT KMS could not do what was asked.
#[derive(Debug, Error)]
pub enum StackitKmsError {
    /// No access token: signing in to STACKIT failed.
    #[error(transparent)]
    SignIn(#[from] SignInError),
    /// The request got no answer, such as for a network error or a timeout.
    #[error("STACKIT KMS could not be reached: {0}")]
    Unreachable(#[source] reqwest::Error),
    /// STACKIT KMS answered with an error. The message is STACKIT's, or the start of the answer.
    #[error("STACKIT KMS refused the request ({status}): {message}")]
    Refused { status: u16, message: String },
    /// STACKIT KMS answered with something its API doesn't describe.
    #[error("STACKIT KMS answered unexpectedly: {0}")]
    Unexpected(String),
    /// A new key got no version that can sign in time.
    #[error("Key `{key_id}` got no version that can sign within {} seconds", waited.as_secs_f32())]
    NoUsableVersion { key_id: String, waited: Duration },
    /// A new key that can't be used couldn't be deleted again, so it is left in the key ring with its label.
    #[error("{cause}; key `{key_id}` could not be deleted again either ({deletion}), so delete it in STACKIT")]
    Abandoned {
        key_id: String,
        cause: Box<StackitKmsError>,
        deletion: Box<StackitKmsError>,
    },
}

/// A key in the key ring.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Key {
    pub(crate) id: String,
    pub(crate) display_name: String,
    algorithm: String,
    purpose: String,
    pub(crate) state: KeyState,
}

impl Key {
    /// Whether the key signs with ES256, the only algorithm the provider signs with.
    pub(crate) fn signs_es256(&self) -> bool {
        self.algorithm == ES256_ALGORITHM && self.purpose == SIGNING_PURPOSE
    }

    /// The key's algorithm, as STACKIT names it.
    pub(crate) fn algorithm(&self) -> &str {
        &self.algorithm
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum KeyState {
    Active,
    /// Scheduled for deletion: STACKIT keeps the key for 30 days, during which it can be restored.
    Deleted,
    /// Being created, without a version, or unavailable.
    #[serde(other)]
    Other,
}

/// A version of a key, which holds its key material.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Version {
    pub(crate) number: u64,
    state: VersionState,
    disabled: bool,
    /// PEM, for asymmetric keys.
    pub(crate) public_key: Option<String>,
}

impl Version {
    /// Whether the version can sign.
    pub(crate) fn is_usable(&self) -> bool {
        self.state == VersionState::Active && !self.disabled
    }

    /// Whether the version's key material is gone for good.
    pub(crate) fn is_destroyed(&self) -> bool {
        self.state == VersionState::Destroyed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VersionState {
    Active,
    Destroyed,
    /// Being created, or its key material is invalid or unavailable.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Keys {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
struct Versions {
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Signature {
    signature: String,
}

/// The STACKIT KMS API for one key ring, signed in to with one service account.
pub(crate) struct KmsApi {
    http: reqwest::Client,
    /// `<KMS>/v1/projects/<project>/regions/<region>/keyrings/<key ring>`.
    key_ring: Url,
    tokens: AccessTokens,
}

impl KmsApi {
    /// The API at `kms_url` for the key ring of `settings`, with access tokens from `tokens`.
    pub(crate) fn new(
        kms_url: &Url,
        settings: &StackitKmsSettings,
        tokens: AccessTokens,
        http: reqwest::Client,
    ) -> Self {
        let mut key_ring = kms_url.clone();
        key_ring
            .path_segments_mut()
            .expect("an HTTP URL")
            .pop_if_empty()
            .extend([
                "v1",
                "projects",
                &settings.project_id,
                "regions",
                &settings.region.to_string(),
                "keyrings",
                &settings.key_ring_id,
            ]);
        Self { http, key_ring, tokens }
    }

    /// The HTTP client the API is reached with, with timeouts for every request.
    ///
    /// # Errors
    ///
    /// When no TLS backend can be set up.
    pub(crate) fn client() -> Result<reqwest::Client, reqwest::Error> {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
    }

    /// Whether the key ring exists.
    pub(crate) async fn key_ring_exists(&self) -> Result<bool, StackitKmsError> {
        Ok(self.request(Method::GET, &[], None).await?.is_some())
    }

    /// Every key in the key ring, deleted ones included. STACKIT doesn't page this list.
    pub(crate) async fn keys(&self) -> Result<Vec<Key>, StackitKmsError> {
        let keys: Option<Keys> = self.decoded(Method::GET, &["keys"], None).await?;
        Ok(keys.map(|keys| keys.keys).unwrap_or_default())
    }

    pub(crate) async fn key(&self, key_id: &str) -> Result<Option<Key>, StackitKmsError> {
        self.decoded(Method::GET, &["keys", key_id], None).await
    }

    /// Creates a P-256 signing key with `software` protection, named `display_name`.
    pub(crate) async fn create_key(&self, display_name: &str) -> Result<Key, StackitKmsError> {
        let body = json!({
            "displayName": display_name,
            "algorithm": ES256_ALGORITHM,
            "purpose": SIGNING_PURPOSE,
            "protection": "software",
        });
        self.decoded(Method::POST, &["keys"], Some(body))
            .await?
            .ok_or_else(|| StackitKmsError::Unexpected("the key ring was not found".to_string()))
    }

    /// Schedules the key's deletion after 30 days. `None` when there is no such key.
    pub(crate) async fn delete_key(&self, key_id: &str) -> Result<Option<()>, StackitKmsError> {
        Ok(self.request(Method::DELETE, &["keys", key_id], None).await?.map(|_| ()))
    }

    /// The key's versions, or `None` when there is no such key.
    pub(crate) async fn versions(&self, key_id: &str) -> Result<Option<Vec<Version>>, StackitKmsError> {
        let versions: Option<Versions> = self.decoded(Method::GET, &["keys", key_id, "versions"], None).await?;
        Ok(versions.map(|versions| versions.versions))
    }

    pub(crate) async fn version(&self, key_id: &str, number: u64) -> Result<Option<Version>, StackitKmsError> {
        self.decoded(Method::GET, &["keys", key_id, "versions", &number.to_string()], None)
            .await
    }

    /// Signs `data`, which STACKIT hashes with SHA-256, with a version of a key. The signature is DER, or `None` when
    /// there is no such key or version.
    pub(crate) async fn sign(
        &self,
        key_id: &str,
        number: u64,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, StackitKmsError> {
        let body = json!({ "data": STANDARD.encode(data) });
        let path = ["keys", key_id, "versions", &number.to_string(), "sign"];
        let Some(signature) = self.decoded::<Signature>(Method::POST, &path, Some(body)).await? else {
            return Ok(None);
        };
        STANDARD
            .decode(signature.signature)
            .map(Some)
            .map_err(|error| StackitKmsError::Unexpected(format!("the signature is not base64: {error}")))
    }

    async fn decoded<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &[&str],
        body: Option<Value>,
    ) -> Result<Option<T>, StackitKmsError> {
        let Some(answer) = self.request(method, path, body).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&answer)
            .map(Some)
            .map_err(|error| StackitKmsError::Unexpected(error.to_string()))
    }

    /// Sends a request for `path` below the key ring, and answers with the body, or `None` for a `404`. A request
    /// STACKIT refuses the access token for is sent once more, with a new one.
    async fn request(
        &self,
        method: Method,
        path: &[&str],
        body: Option<Value>,
    ) -> Result<Option<Vec<u8>>, StackitKmsError> {
        let mut url = self.key_ring.clone();
        url.path_segments_mut().expect("an HTTP URL").extend(path);

        let token = self.tokens.get().await?;
        let mut response = self.send(&method, &url, body.as_ref(), &token).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.tokens.forget(&token).await;
            let token = self.tokens.get().await?;
            response = self.send(&method, &url, body.as_ref(), &token).await?;
        }

        let status = response.status();
        let answer = response.bytes().await.map_err(StackitKmsError::Unreachable)?;
        match status {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => Ok(Some(answer.to_vec())),
            status => Err(StackitKmsError::Refused {
                status: status.as_u16(),
                message: refusal_message(&String::from_utf8_lossy(&answer)),
            }),
        }
    }

    async fn send(
        &self,
        method: &Method,
        url: &Url,
        body: Option<&Value>,
        token: &str,
    ) -> Result<reqwest::Response, StackitKmsError> {
        let mut request = self.http.request(method.clone(), url.clone()).bearer_auth(token);
        if let Some(body) = body {
            request = request.json(body);
        }
        request.send().await.map_err(StackitKmsError::Unreachable)
    }
}
