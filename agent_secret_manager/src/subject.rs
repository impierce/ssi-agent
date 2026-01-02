use crate::stronghold_storage;
use agent_shared::config::{config, SupportedDidMethod};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager_consumer::resolver::Resolver;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::did::DIDUrl;
use identity_iota::storage::{JwkStorage, KeyId};
use identity_iota::verification::jwk::Jwk;
use identity_iota::{did::DID, document::DIDUrlQuery, verification::jwk::JwkParams};
use jsonwebtoken::Algorithm;
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Reponsible for signing and verifying data.
#[derive(Debug)]
pub struct Subject {
    pub stronghold_storage: StrongholdExtStorage,
    pub verification_method_ids: Arc<Mutex<HashMap<StorageKey, DIDUrl>>>,
    pub resolver: Resolver,
}

impl Subject {
    /// Create a new Subject.
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;

        Self {
            stronghold_storage,
            verification_method_ids: Arc::new(Mutex::new(HashMap::new())),
            resolver: Resolver::new(None, None).await,
        }
    }

    pub async fn configure_resolver(&mut self, node_url: Option<&str>, tls_config: Option<rustls::ClientConfig>) {
        self.resolver = Resolver::new(node_url, tls_config).await;
    }

    pub async fn get_public_key(&self, key_id: KeyId, algorithm: &Algorithm) -> anyhow::Result<Jwk> {
        match algorithm {
            Algorithm::EdDSA => self.stronghold_storage.get_ed25519_public_key(&key_id).await,
            Algorithm::ES256 => self.stronghold_storage.get_es256_public_key(&key_id).await,
            _ => anyhow::bail!("Unsuported algorithm"),
        }
        .map_err(Into::into)
    }

    pub async fn insert_verification_method_id(
        &self,
        key: StorageKey,
        verification_method_id: DIDUrl,
    ) -> anyhow::Result<()> {
        self.verification_method_ids
            .lock()
            .await
            .insert(key, verification_method_id);
        Ok(())
    }

    pub async fn get_verification_method_id(&self, key: StorageKey) -> Option<DIDUrl> {
        self.verification_method_ids.lock().await.get(&key).cloned()
    }
}

#[async_trait]
pub trait SubjectExt: oid4vc_core::Subject {
    async fn resolve_public_key(&self, did_url: &str) -> anyhow::Result<Jwk>;
}

/// Extension trait for `Subject` to provide additional functionality.
#[async_trait]
impl SubjectExt for Subject {
    /// Resolves the public key for a given DID URL.
    async fn resolve_public_key(&self, did_url: &str) -> anyhow::Result<Jwk> {
        let did_url =
            identity_iota::did::DIDUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

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

        verification_method
            .data()
            .public_key_jwk()
            .ok_or_else(|| anyhow!("Failed to resolve public key for DID URL: `{did_url}`"))
            .cloned()
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

    // This `Default` implementation for `Subject` returns a new `Subject` with the Verification Method IDs already preloaded.
    impl Default for Subject {
        fn default() -> Self {
            futures::executor::block_on(async {
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
                    stronghold_storage,
                    verification_method_ids,
                    resolver: Resolver::new(None, None).await,
                }
            })
        }
    }
}

#[async_trait]
impl Verify for Subject {
    async fn public_key(&self, did_url: &str) -> anyhow::Result<Vec<u8>> {
        let did_url =
            identity_iota::did::DIDUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

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

        // Try decode from `MethodData` directly, else use public JWK params.
        verification_method.data().try_decode().or_else(|_| {
            verification_method
                .data()
                .public_key_jwk()
                .and_then(|public_key_jwk| match public_key_jwk.params() {
                    JwkParams::Okp(okp_params) => URL_SAFE_NO_PAD.decode(&okp_params.x).ok(),
                    JwkParams::Ec(ec_params) => {
                        let x_bytes = URL_SAFE_NO_PAD.decode(&ec_params.x).ok()?;
                        let y_bytes = URL_SAFE_NO_PAD.decode(&ec_params.y).ok()?;

                        let encoded_point = p256::EncodedPoint::from_affine_coordinates(
                            p256::FieldBytes::from_slice(&x_bytes),
                            p256::FieldBytes::from_slice(&y_bytes),
                            false, // false for uncompressed point
                        );

                        let verifying_key = p256::ecdsa::VerifyingKey::from_encoded_point(&encoded_point)
                            .expect("Failed to create verifying key from encoded point");

                        Some(verifying_key.to_encoded_point(false).as_bytes().to_vec())
                    }
                    _ => None,
                })
                .ok_or(anyhow!("Failed to decode public key for DID URL: `{did_url}`"))
        })
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
        let stronghold_storage = &self.stronghold_storage;
        let (key_id, public_key) = match algorithm {
            Algorithm::ES256 => {
                let es256_key_id = config().secret_manager.issuer_es256_key_id.clone();
                let public_key = stronghold_storage.get_es256_public_key(&es256_key_id).await?;
                (es256_key_id, public_key)
            }
            Algorithm::EdDSA => {
                let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();
                let public_key = stronghold_storage.get_ed25519_public_key(&ed25519_key_id).await?;
                (ed25519_key_id, public_key)
            }
            _ => return Err(anyhow!("Unsupported algorithm")),
        };

        stronghold_storage
            .sign(&key_id, message.as_bytes(), &public_key)
            .await
            .map_err(Into::into)
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
            .map(DIDUrl::did)
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
    use agent_shared::config::{
        default_issuer_eddsa_key_id, default_issuer_es256_key_id, set_config, SecretManagerConfig,
    };
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
        set_config().set_secret_manager_config(SECRET_MANAGER_CONFIG.clone());

        let subject = Arc::new(Subject::default());

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
        set_config().set_secret_manager_config(SECRET_MANAGER_CONFIG.clone());

        let subject = Arc::new(Subject::default());

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

    #[tokio::test]
    async fn test_configure_resolver() {
        let mut subject = Subject::new().await;
        // TODO: find a node_url for testing
        subject.configure_resolver(None, None).await;

        subject
            .resolver
            .resolve("did:key:z6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt")
            .await
            .unwrap();
        subject
            .resolver
            .resolve("did:iota:testnet:0x04b26f82ba06c22a3ed57069cc349239bccd972fbb24ac5a7e0db6a0b9c42292")
            .await
            .unwrap();
    }
}
