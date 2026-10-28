use crate::key_store::{issuer_keys, StrongholdKeyStore};
use crate::stronghold_storage;
use agent_shared::config::{config, SupportedDidMethod};
use anyhow::anyhow;
use async_trait::async_trait;
use did_manager_consumer::resolver::Resolver;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use did_manager_iota::consumer::NodeUrls;
use identity_iota::did::DIDUrl;
use identity_iota::storage::KeyId;
use identity_iota::{did::DID, document::DIDUrlQuery};
use identity_kit::{
    did::{DidUrl, VerificationRelationship},
    jose::{Jwk, JwsAlgorithm, Signer as _},
    keys::{KeySigner, KeyStore},
    methods::{jwk::DidJwk, key::DidKey, web::DidWeb, web::ReqwestClient},
    resolver::{KeyReference, KeyResolutionError, KeyResolver, KeyRole},
};
use jsonwebtoken::Algorithm;
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Responsible for signing and verifying data.
pub struct Subject {
    pub stronghold_storage: StrongholdExtStorage,
    /// Holds the configured issuer keys.
    pub key_store: Arc<dyn KeyStore>,
    pub verification_method_ids: Arc<Mutex<HashMap<StorageKey, DidUrl>>>,
    /// Resolves `did:key`, `did:jwk` and `did:web`.
    pub key_resolver: identity_kit::resolver::Resolver,
    /// Resolves the DID methods `key_resolver` doesn't.
    pub resolver: Resolver,
}

impl fmt::Debug for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subject")
            .field("stronghold_storage", &self.stronghold_storage)
            .field("verification_method_ids", &self.verification_method_ids)
            .field("key_resolver", &self.key_resolver)
            .field("resolver", &self.resolver)
            .finish_non_exhaustive()
    }
}

/// The resolver for `did:key`, `did:jwk` and `did:web`, which fetches `did:web` documents with IdentityKit's hardened
/// HTTP client. With the `allow-localhost` feature, that client may also fetch them from the local network over plain
/// `http`, like the agent's other outbound requests; see
/// `docs/adr/0002-allow-localhost-http-fallback-for-local-testing.md`.
fn key_resolver() -> identity_kit::resolver::Resolver {
    #[cfg(not(feature = "allow-localhost"))]
    let client = ReqwestClient::new();
    #[cfg(feature = "allow-localhost")]
    let client = local_network_client();

    resolver_with(client.expect("Failed to initialize the HTTP client for `did:web`"))
}

/// IdentityKit's HTTP client, which may also reach the local network over plain `http`.
#[cfg(any(feature = "allow-localhost", feature = "test_utils"))]
fn local_network_client() -> Result<ReqwestClient, identity_kit::methods::web::HttpError> {
    ReqwestClient::builder()
        .dangerous_allow_loopback()
        .dangerous_allow_private_network()
        .build()
}

fn resolver_with(client: ReqwestClient) -> identity_kit::resolver::Resolver {
    identity_kit::resolver::Resolver::builder()
        .with(DidKey::new())
        .with(DidJwk)
        .with(DidWeb::new(client))
        .build()
}

/// The configured issuer key that signs with `algorithm`.
fn issuer_key(algorithm: Algorithm) -> anyhow::Result<(identity_kit::keys::KeyId, JwsAlgorithm)> {
    let alg = JwsAlgorithm::try_from(algorithm)?;

    issuer_keys()
        .into_iter()
        .find(|(_, key_alg)| *key_alg == alg)
        .ok_or_else(|| anyhow!("No issuer key signs with {alg}"))
}

impl Subject {
    /// Create a new Subject.
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        // TODO: all networks are set to the same node URL ==> introduce per-network configuration?
        let node_urls = NodeUrls {
            mainnet: config().iota_node_url.clone(),
            testnet: config().iota_node_url.clone(),
            devnet: config().iota_node_url.clone(),
        };

        let username_password = config()
            .iota_node_username
            .clone()
            .zip(config().iota_node_password.clone());
        let username_password = username_password.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));

        Self {
            key_store: Arc::new(StrongholdKeyStore::issuer_keys(stronghold_storage.clone())),
            stronghold_storage,
            verification_method_ids: Arc::new(Mutex::new(HashMap::new())),
            key_resolver: key_resolver(),
            resolver: Resolver::new_with_options(None, Some(node_urls), username_password),
        }
    }

    /// IdentityKit's signer with the configured issuer key for `algorithm`.
    pub async fn issuer_signer(&self, algorithm: Algorithm) -> anyhow::Result<KeySigner<Arc<dyn KeyStore>>> {
        let (key, alg) = issuer_key(algorithm)?;

        Ok(KeySigner::new(Arc::clone(&self.key_store), key, alg).await?)
    }

    pub async fn get_public_key(&self, key_id: KeyId) -> anyhow::Result<Jwk> {
        Ok(self
            .key_store
            .public_jwk(&identity_kit::keys::KeyId::new(key_id.as_str()))
            .await?)
    }

    pub async fn insert_verification_method_id(
        &self,
        key: StorageKey,
        verification_method_id: DidUrl,
    ) -> anyhow::Result<()> {
        self.verification_method_ids
            .lock()
            .await
            .insert(key, verification_method_id);
        Ok(())
    }

    pub async fn get_verification_method_id(&self, key: StorageKey) -> Option<DidUrl> {
        self.verification_method_ids.lock().await.get(&key).cloned()
    }

    /// Resolves the public key for a given DID URL.
    pub async fn resolve_public_key(&self, did_url: &str) -> anyhow::Result<Jwk> {
        let did_url = DidUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

        self.resolve_verification_method_key(&did_url, None).await
    }

    /// Resolves the public key of the verification method `did_url`, which must be usable for `relationship` when one
    /// is given.
    async fn resolve_verification_method_key(
        &self,
        did_url: &DidUrl,
        relationship: Option<VerificationRelationship>,
    ) -> anyhow::Result<Jwk> {
        if self.key_resolver.supports(did_url.did().method()) {
            return self
                .key_resolver
                .resolve_public_key(did_url, relationship)
                .await
                .map_err(|err| anyhow!("Failed to resolve public key for DID URL: `{did_url}`, error: {err}"));
        }

        let did_url = DIDUrl::parse(did_url.as_ref()).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

        let document = self
            .resolver
            .resolve(did_url.did().as_str())
            .await
            .map_err(|err| anyhow!("Failed to resolve DID Document for DID: `{did_url}`, error: {err}"))?;

        let verification_method = document
            .resolve_method(DIDUrlQuery::from(&did_url), None)
            .ok_or(anyhow!(
                "Failed to resolve verification method for DID URL: `{did_url}`"
            ))?;

        let public_key_jwk = verification_method
            .data()
            .public_key_jwk()
            .ok_or_else(|| anyhow!("Failed to resolve public key for DID URL: `{did_url}`"))?;

        Ok(serde_json::from_value(serde_json::to_value(public_key_jwk)?)?)
    }
}

#[async_trait]
impl KeyResolver for Subject {
    async fn resolve_key(&self, key: &KeyReference, role: KeyRole) -> Result<Jwk, KeyResolutionError> {
        let KeyReference::DidUrl(did_url) = key else {
            return Err(KeyResolutionError::unsupported(key));
        };

        self.resolve_verification_method_key(did_url, Some(role.relationship()))
            .await
            .map_err(KeyResolutionError::msg)
    }
}

/// This module contains implementations for `Subject` for testing purposes.
/// It is only available when the `test_utils` feature is enabled.
#[cfg(feature = "test_utils")]
mod default_subject {
    use super::*;

    impl Subject {
        const DID_KEY_ES256_VERIFICATION_METHOD_ID: &str = "did:key:zDnaeRwT4g6AZCHzxvNL7DLjqTaT88am4XR6TUGrKr6DXj6Tz#zDnaeRwT4g6AZCHzxvNL7DLjqTaT88am4XR6TUGrKr6DXj6Tz";
        const DID_KEY_EDDSA_VERIFICATION_METHOD_ID: &str =
            "did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt#z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt";
        const DID_JWK_ES256_VERIFICATION_METHOD_ID: &str =
            "did:jwk:eyJhbGciOiJFUzI1NiIsImNydiI6IlAtMjU2Iiwia2lkIjoib09ZMmRNVlU3R0s1YWwxcTdFQXh1b1lsb3hNUWx2NVpOWk9hdGlVWFFIZyIsImt0eSI6IkVDIiwieCI6IkZtazEzZ08yU0dMYnVYZUwyNHFKUEhDTm5jbkk2bEJ1NlpRTDJFVlp2NEUiLCJ5IjoiZnoyS3ZNaHVmelVwTWVMOS1LMnJlOWZ3QTNtemcxYnBmYmNlSVFTdWloWSJ9#0";
        const DID_JWK_EDDSA_VERIFICATION_METHOD_ID: &str =
            "did:jwk:eyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJiUUtRUnphb3A3Q2dFdnFWcThVbGdMR3NkRi1SLWhuTEZrS0ZacVcyVk4wIiwia3R5IjoiT0tQIiwieCI6Ikdsbks5ZVBzODAyWHhBZ2xST1F6b0d1cm05UXB2MElGUEViZE1DSUxOX1UifQ#0";
    }

    impl Subject {
        /// This subject, resolving `did:web` documents on the local network over plain `http` as well, for tests
        /// that serve them from mock servers.
        pub fn with_local_network_resolver(mut self) -> Self {
            self.key_resolver =
                resolver_with(local_network_client().expect("Failed to initialize the HTTP client for `did:web`"));
            self
        }
    }

    // This `Default` implementation for `Subject` returns a new `Subject` with the Verification Method IDs already preloaded.
    impl Subject {
        pub async fn test_subject() -> Self {
            let stronghold_storage = stronghold_storage().await;

            let verification_method_ids = Arc::new(Mutex::new(HashMap::from_iter(vec![
                (
                    StorageKey::new(SupportedDidMethod::Key, Algorithm::ES256),
                    Self::DID_KEY_ES256_VERIFICATION_METHOD_ID.parse().unwrap(),
                ),
                (
                    StorageKey::new(SupportedDidMethod::Key, Algorithm::EdDSA),
                    Self::DID_KEY_EDDSA_VERIFICATION_METHOD_ID.parse().unwrap(),
                ),
                (
                    StorageKey::new(SupportedDidMethod::Jwk, Algorithm::ES256),
                    Self::DID_JWK_ES256_VERIFICATION_METHOD_ID.parse().unwrap(),
                ),
                (
                    StorageKey::new(SupportedDidMethod::Jwk, Algorithm::EdDSA),
                    Self::DID_JWK_EDDSA_VERIFICATION_METHOD_ID.parse().unwrap(),
                ),
            ])));

            Self {
                key_store: Arc::new(StrongholdKeyStore::issuer_keys(stronghold_storage.clone())),
                stronghold_storage,
                verification_method_ids,
                key_resolver: key_resolver(),
                resolver: Resolver::new(),
            }
        }
    }
}

#[async_trait]
impl Verify for Subject {
    async fn public_key(&self, did_url: &str) -> anyhow::Result<Vec<u8>> {
        let public_key_jwk = self.resolve_public_key(did_url).await?;

        match public_key_jwk.kty() {
            "OKP" => Ok(public_key_jwk.ed25519_public_bytes()?.to_vec()),
            "EC" => Ok(public_key_jwk.p256_public_bytes()?.to_vec()),
            kty => Err(anyhow!("Unsupported key type `{kty}` for DID URL: `{did_url}`")),
        }
    }
}

#[async_trait]
impl Sign for Subject {
    async fn key_id(&self, subject_syntax_type: &str, algorithm: Algorithm) -> Option<String> {
        let method = SupportedDidMethod::from_str(subject_syntax_type).ok()?;

        self.get_verification_method_id(StorageKey::new(method, algorithm))
            .await
            .as_ref()
            .map(ToString::to_string)
    }

    async fn sign(&self, message: &str, _subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<Vec<u8>> {
        Ok(self.issuer_signer(algorithm).await?.sign(message.as_bytes()).await?)
    }

    fn external_signer(&self) -> Option<Arc<dyn ExternalSign>> {
        None
    }
}

#[async_trait]
impl oid4vc_core::Subject for Subject {
    async fn identifier(&self, subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<String> {
        let method = SupportedDidMethod::from_str(subject_syntax_type)
            .map_err(|e| anyhow!("Failed to parse SupportedDidMethod from string: {}", e))?;

        self.get_verification_method_id(StorageKey::new(method, algorithm))
            .await
            .as_ref()
            .map(DidUrl::did)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("Failed to get verification method ID"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StorageKey {
    pub did_method: SupportedDidMethod,
    pub algorithm: Algorithm,
}

impl StorageKey {
    pub fn new(did_method: SupportedDidMethod, algorithm: Algorithm) -> Self {
        Self { did_method, algorithm }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::config::{default_issuer_eddsa_key_id, default_issuer_es256_key_id, SecretManagerConfig};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED, ED25519};

    const ES256_SIGNED_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGVXpJMU5pSXNJbU55ZGlJNklsQXRNalUySWl3aWEybGtJam9pTkVGMVdXaFNRMk5HYkc0eWJuUm5VMTlxT1hCRlFtUkxkekl3VUhRdGJHRnFXVWh0V1RkQk1FMUdUU0lzSW10MGVTSTZJa1ZESWl3aWVDSTZJakpNV0dwT1JFOTZWM1J3WlZOWk0ydGlUbEkyWm14YVRVUjRZV2gxYXpKMlVXMWpkWFprUVRodk5EUWlMQ0o1SWpvaVpFRjJSVlpzV0UxSFVFdGFjMnRXV1RSWlZ6QnpPRUk0UzNZM2Myc3hZemt5VDA1WVJFcHZlRjlJY3lKOSMwIn0.eyJpc3MiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaU5FRjFXV2hTUTJOR2JHNHliblJuVTE5cU9YQkZRbVJMZHpJd1VIUXRiR0ZxV1VodFdUZEJNRTFHVFNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SWpKTVdHcE9SRTk2VjNSd1pWTlpNMnRpVGxJMlpteGFUVVI0WVdoMWF6SjJVVzFqZFhaa1FUaHZORFFpTENKNUlqb2laRUYyUlZac1dFMUhVRXRhYzJ0V1dUUlpWekJ6T0VJNFMzWTNjMnN4WXpreVQwNVlSRXB2ZUY5SWN5SjkiLCJzdWIiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaU5FRjFXV2hTUTJOR2JHNHliblJuVTE5cU9YQkZRbVJMZHpJd1VIUXRiR0ZxV1VodFdUZEJNRTFHVFNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SWpKTVdHcE9SRTk2VjNSd1pWTlpNMnRpVGxJMlpteGFUVVI0WVdoMWF6SjJVVzFqZFhaa1FUaHZORFFpTENKNUlqb2laRUYyUlZac1dFMUhVRXRhYzJ0V1dUUlpWekJ6T0VJNFMzWTNjMnN4WXpreVQwNVlSRXB2ZUY5SWN5SjkiLCJhdWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaVlrNDNiSEpaWVhOUlZrNDNMVUpZY0MxMFdFVldTR1l0YVhkTWRsVnRiWHByVUZsc2VHWlRWRkZvVlNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SW1odVkyNU5UM2sxU0dGWGJ6SmFTbmhCWW5sWU1GOW1NVTFHU1dsMlRrRmtUMjFXYjNSWGVWZG9ielFpTENKNUlqb2libE5wYkhwMllsTmFYMUp1VWpOU2RreHdkRWxITmpkVWJWVkVhR1ZQWVZGNlltczJhVFJmWDBkeVFTSjkiLCJleHAiOjE3MjMwMjkyMjUsImlhdCI6MTcyMzAyODYyNSwibm9uY2UiOiJ0aGlzIGlzIGEgbm9uY2UifQ.w202CZKOeGM9k35tysJylksBUGI3fvkOgsPPVrfXYZzurns7KF5plMiR_KHH4H_GpYg57Nf2JWa3YEcXGDTVdw";
    const EDDSA_SIGNED_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKSmJWOVpNRkZQTm05SFgyczVNbTlzY1RWTWRIUTJZVkE0YzE5QmJFRmhWVUl6UzBkelVFY3RlR0kwSWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWxaUGFrUjBRblozY0daalNraHlUelpMVjFOUGRYTlZVR1ptUWt3eVIxOUtjWFp0VVRZNFMzaDRWalFpZlEjMCJ9.eyJpc3MiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWRpSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSkpiVjlaTUZGUE5tOUhYMnM1TW05c2NUVk1kSFEyWVZBNGMxOUJiRUZoVlVJelMwZHpVRWN0ZUdJMElpd2lhM1I1SWpvaVQwdFFJaXdpZUNJNklsWlBha1IwUW5aM2NHWmpTa2h5VHpaTFYxTlBkWE5WVUdabVFrd3lSMTlLY1hadFVUWTRTM2g0VmpRaWZRIiwic3ViIjoiZGlkOmp3azpleUpoYkdjaU9pSkZaRVJUUVNJc0ltTnlkaUk2SWtWa01qVTFNVGtpTENKcmFXUWlPaUpKYlY5Wk1GRlBObTlIWDJzNU1tOXNjVFZNZEhRMllWQTRjMTlCYkVGaFZVSXpTMGR6VUVjdGVHSTBJaXdpYTNSNUlqb2lUMHRRSWl3aWVDSTZJbFpQYWtSMFFuWjNjR1pqU2toeVR6WkxWMU5QZFhOVlVHWm1Ra3d5UjE5S2NYWnRVVFk0UzNoNFZqUWlmUSIsImF1ZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKdFFqSXhUV2t5Y1V0WVZtTTFOREpVWWt0U09UZ3lUelpUWjFKWVZrWlFaVzV3TTNGWWRIRlRla3R2SWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWprM1JVRXpSSE5vUmpONlIwSllTVjlVYnpObVJrUnJNVTFxV1VaYVV6bFZiMUpVYmxCT1NIUlpVV01pZlEiLCJleHAiOjE3MjMwMzE3MTQsImlhdCI6MTcyMzAzMTExNCwibm9uY2UiOiJ0aGlzIGlzIGEgbm9uY2UifQ.oGRYpwH4QvWZs0bZkgAuxq6MqNYdoX44KxNfRl7GzXCnv_0D_c19rhYMwzn04R7udNCthFDr7GUhXLQgROlUDw";

    lazy_static::lazy_static! {
        static ref SECRET_MANAGER_CONFIG: SecretManagerConfig = SecretManagerConfig {
            stronghold_password: "sup3rSecr3t".to_string(),
            stronghold_path: "/tmp/stronghold".to_string(),
            issuer_eddsa_key_id: default_issuer_eddsa_key_id(),
            issuer_es256_key_id: default_issuer_es256_key_id(),
        };
    }

    #[tokio::test]
    async fn es256_signed_jwt_successfully_verified() {
        let subject = Arc::new(Subject::test_subject().await);

        let mut split = ES256_SIGNED_JWT.rsplitn(2, '.');
        let (signature, message) = (split.next().unwrap(), split.next().unwrap());

        // Decode the signature.
        let signature_bytes = URL_SAFE_NO_PAD.decode(signature).unwrap();

        // Resolve the public key from the DID Document
        let public_key_bytes = subject.public_key("did:jwk:eyJhbGciOiJFUzI1NiIsImNydiI6IlAtMjU2Iiwia2lkIjoiNEF1WWhSQ2NGbG4ybnRnU19qOXBFQmRLdzIwUHQtbGFqWUhtWTdBME1GTSIsImt0eSI6IkVDIiwieCI6IjJMWGpORE96V3RwZVNZM2tiTlI2ZmxaTUR4YWh1azJ2UW1jdXZkQThvNDQiLCJ5IjoiZEF2RVZsWE1HUEtac2tWWTRZVzBzOEI4S3Y3c2sxYzkyT05YREpveF9IcyJ9#0").await.unwrap();

        // Verify the signature
        let public_key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key_bytes);
        assert!(public_key.verify(message.as_bytes(), &signature_bytes).is_ok());
    }

    #[tokio::test]
    async fn eddsa_signed_jwt_successfully_verified() {
        let subject = Arc::new(Subject::test_subject().await);

        let mut split = EDDSA_SIGNED_JWT.rsplitn(2, '.');
        let (signature, message) = (split.next().unwrap(), split.next().unwrap());

        // Decode the signature.
        let signature_bytes = URL_SAFE_NO_PAD.decode(signature).unwrap();

        // Resolve the public key from the DID Document
        let public_key_bytes = subject.public_key("did:jwk:eyJhbGciOiJFZERTQSIsImNydiI6IkVkMjU1MTkiLCJraWQiOiJJbV9ZMFFPNm9HX2s5Mm9scTVMdHQ2YVA4c19BbEFhVUIzS0dzUEcteGI0Iiwia3R5IjoiT0tQIiwieCI6IlZPakR0QnZ3cGZjSkhyTzZLV1NPdXNVUGZmQkwyR19KcXZtUTY4S3h4VjQifQ#0").await.unwrap();

        // Verify the signature
        let public_key = UnparsedPublicKey::new(&ED25519, public_key_bytes);
        assert!(public_key.verify(message.as_bytes(), &signature_bytes).is_ok());
    }

    /// The public key of the `did:web` document [`serve_did_web`] serves.
    fn did_web_public_key() -> serde_json::Value {
        serde_json::json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        })
    }

    /// Serves a `did:web` document on `localhost` and returns the DID URL of its verification method.
    async fn serve_did_web() -> (wiremock::MockServer, String) {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };

        let server = MockServer::start().await;
        let did = format!("did:web:localhost%3A{}", server.address().port());
        let did_url = format!("{did}#key-0");
        let document = serde_json::json!({
            "id": did,
            "verificationMethod": [{
                "id": did_url,
                "type": "JsonWebKey2020",
                "controller": did,
                "publicKeyJwk": did_web_public_key()
            }],
            "assertionMethod": [did_url]
        });
        Mock::given(method("GET"))
            .and(path("/.well-known/did.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(document))
            .mount(&server)
            .await;

        (server, did_url)
    }

    #[cfg(not(feature = "allow-localhost"))]
    #[tokio::test]
    async fn does_not_resolve_did_web_on_the_local_network() {
        let subject = Subject::test_subject().await;
        let (server, did_url) = serve_did_web().await;

        assert!(subject.resolve_public_key(&did_url).await.is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[cfg(feature = "allow-localhost")]
    mod allow_localhost_tests {
        use super::*;

        #[tokio::test]
        async fn resolves_did_web_on_the_local_network() {
            let subject = Subject::test_subject().await;
            let (_server, did_url) = serve_did_web().await;

            let public_key = subject.resolve_public_key(&did_url).await.unwrap();

            assert_eq!(serde_json::json!(public_key), did_web_public_key());
        }
    }
}
