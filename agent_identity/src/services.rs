use agent_secret_manager::{
    managed_key::{self, aggregate::SigningAlgorithm},
    services::SecretManagerServices,
    state::SecretManagerState,
};
use agent_shared::{
    config::{config, SupportedDidMethod},
    handlers::query_handler,
};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager_consumer::resolver::Resolver;
use identity_credential::domain_linkage::{DomainLinkageConfiguration, DomainLinkageCredentialBuilder};
use identity_did::{DIDUrl, DID as _};
use identity_iota::{
    core::{Duration, Timestamp, ToJson},
    credential::Jwt,
    document::DIDUrlQuery,
    verification::jwk::{Jwk, JwkParams},
};
use identity_storage::{JwkStorage as _, KeyId};
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};
use std::{
    str::FromStr as _,
    sync::{Arc, OnceLock},
};

use crate::state::IdentityState;

/// Identity services.
pub struct IdentityServices {
    pub secret_manager_services: Arc<SecretManagerServices>,
}

impl IdentityServices {
    pub fn new(secret_manager_services: Arc<SecretManagerServices>) -> Self {
        Self {
            secret_manager_services,
        }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        Arc::new(Self::new(Arc::new(futures::executor::block_on(async {
            SecretManagerServices::new().await
        }))))
    }
}

pub static GLOBAL_SERVICE: OnceLock<Arc<ThisIsTheMainService>> = OnceLock::new();

pub struct ThisIsTheMainService {
    pub secret_manager_state: Arc<SecretManagerState>,
    pub secret_manager_services: Arc<SecretManagerServices>,
    pub identity_state: Arc<IdentityState>,
    pub resolver: Resolver,
}

impl ThisIsTheMainService {
    pub async fn new(
        secret_manager_state: Arc<SecretManagerState>,
        secret_manager_services: Arc<SecretManagerServices>,
        identity_state: Arc<IdentityState>,
    ) -> Self {
        let resolver = Resolver::new().await;

        Self {
            secret_manager_state,
            secret_manager_services,
            identity_state,
            resolver,
        }
    }

    pub async fn create_domain_linkage_configuration(&self) -> anyhow::Result<DomainLinkageConfiguration> {
        let stronghold_storage = &self.secret_manager_services.stronghold_storage;

        let origin = identity_core::common::Url::parse(config().public_url.origin().ascii_serialization()).unwrap();
        // .map_err(|err| InvalidUrlError(err.to_string()))?;

        // #[cfg(feature = "test_utils")]
        // let (issuance_date, expiration_date) = {
        //     let issuance_date = test_utils::issuance_date();
        //     let expiration_date = test_utils::expiration_date();
        //     (issuance_date, expiration_date)
        // };
        // #[cfg(not(feature = "test_utils"))]
        let (issuance_date, expiration_date) = {
            let issuance_date = Timestamp::now_utc();
            let expiration_date = issuance_date
                // TODO: make this configurable
                .checked_add(Duration::days(365))
                .unwrap();
            // .ok_or(InvalidTimestampError)?;

            (issuance_date, expiration_date)
        };

        let all_documents_view = query_handler("all_documents", &self.identity_state.query.all_documents)
            .await
            .unwrap()
            .unwrap();

        let did_method_and_verification_method_ids = all_documents_view
            .documents
            .into_values()
            .filter_map(|document_view| {
                Some((document_view.did_method.unwrap(), document_view.verification_method_ids))
            })
            .collect::<Vec<_>>();

        let mut linked_dids = vec![];

        for (_did_method, verification_method_ids) in did_method_and_verification_method_ids {
            for (key_id, verification_method_id) in verification_method_ids {
                let key_id = KeyId::new(&key_id);

                // FIXME: terrible temporary logic to determine key type
                let public_key = if let Ok(public_key) = stronghold_storage.get_es256_public_key(&key_id).await {
                    public_key
                } else {
                    stronghold_storage.get_ed25519_public_key(&key_id).await?
                };

                let subject_did = verification_method_id.did();
                let algorithm = Algorithm::from_str(public_key.alg().unwrap()).unwrap();

                let domain_linkage_credential = DomainLinkageCredentialBuilder::new()
                    .issuer(subject_did.clone())
                    .origin(origin.clone())
                    .issuance_date(issuance_date)
                    .expiration_date(expiration_date)
                    .build()
                    .unwrap()
                    // .map_err(|err| DomainLinkageCredentialBuilderError(err.to_string()))?
                    .serialize_jwt(Default::default())
                    .unwrap();
                // .map_err(|err| SerializationError(err.to_string()))?;

                // Compose JWT
                let header = Header {
                    alg: algorithm,
                    typ: None,
                    kid: Some(verification_method_id.to_string()),
                    ..Default::default()
                };

                let linked_did = [
                    URL_SAFE_NO_PAD.encode(
                        header.to_json_vec().unwrap(), // .map_err(|err| SerializationError(err.to_string()))?,
                    ),
                    URL_SAFE_NO_PAD.encode(domain_linkage_credential.as_bytes()),
                ]
                .join(".");

                stronghold_storage
                    .sign(&key_id, linked_did.as_bytes(), &public_key)
                    .await
                    .unwrap();

                linked_dids.push(Jwt::from(linked_did));
            }
        }

        if linked_dids.is_empty() {
            return Err(anyhow!("No linked DIDs found"));
        }

        Ok(DomainLinkageConfiguration::new(linked_dids))
    }
}

impl std::fmt::Debug for ThisIsTheMainService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThisIsTheMainService").finish()
    }
}

#[async_trait]
pub trait SubjectExt: oid4vc_core::Subject {
    async fn resolve_public_key(&self, did_url: &str) -> anyhow::Result<Jwk>;
}

/// Extension trait for `Subject` to provide additional functionality.
#[async_trait]
impl SubjectExt for ThisIsTheMainService {
    /// Resolves the public key for a given DID URL.
    async fn resolve_public_key(&self, did_url: &str) -> anyhow::Result<Jwk> {
        let did_url =
            identity_iota::did::DIDUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

        let resolver = &self.resolver;

        let document = resolver
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

// /// This module contains implementations for `Subject` for testing purposes.
// /// It is only available when the `test_utils` feature is enabled.
// #[cfg(feature = "test_utils")]
// mod default_subject {
//     use super::*;

//     // This `Default` implementation for `Subject` returns a new `Subject` with the Verification Method IDs already preloaded.
//     impl Default for Subject {
//         fn default() -> Self {
//             futures::executor::block_on(async {
//                 let stronghold_storage = stronghold_storage().await;

//                 Self { stronghold_storage }
//             })
//         }
//     }
// }

#[async_trait]
impl Verify for ThisIsTheMainService {
    async fn public_key(&self, did_url: &str) -> anyhow::Result<Vec<u8>> {
        let did_url =
            identity_iota::did::DIDUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

        // TODO: Make sure the resolver only needs to be created once.
        let resolver = Resolver::new().await;

        let document = resolver
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
impl Sign for ThisIsTheMainService {
    async fn key_id(&self, subject_syntax_type: &str, algorithm: Algorithm) -> Option<String> {
        let method = SupportedDidMethod::from_str(subject_syntax_type).ok()?;

        let signing_algorithm: SigningAlgorithm = match algorithm {
            Algorithm::EdDSA => SigningAlgorithm::EdDSA,
            Algorithm::ES256 => SigningAlgorithm::ES256,
            _ => return None,
        };

        let all_managed_keys_view =
            query_handler("all_managed_keys", &self.secret_manager_state.query.all_managed_keys)
                .await
                .unwrap()
                .unwrap();

        let managed_key_view = all_managed_keys_view
            .managed_keys
            .values()
            .find(|managed_key_view| {
                managed_key_view.signing_algorithm == Some(signing_algorithm.clone())
                    && managed_key_view.is_signing_key
                    && !managed_key_view.is_removed
            })
            .unwrap();

        let all_documents_view = query_handler("all_documents", &self.identity_state.query.all_documents)
            .await
            .unwrap()
            .unwrap();

        let document_view = all_documents_view
            .documents
            .values()
            .find(|document_view| document_view.did_method == Some(method))
            .unwrap();

        document_view
            .verification_method_ids
            .iter()
            .find_map(|(key_id, verification_method_id)| {
                if *key_id == managed_key_view.key_id {
                    Some(verification_method_id.to_string())
                } else {
                    None
                }
            })
    }

    async fn sign(&self, message: &str, _subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<Vec<u8>> {
        let stronghold_storage = &self.secret_manager_services.stronghold_storage;

        let signing_algorithm: SigningAlgorithm = match algorithm {
            Algorithm::EdDSA => SigningAlgorithm::EdDSA,
            Algorithm::ES256 => SigningAlgorithm::ES256,
            _ => todo!(),
        };

        let all_managed_keys_view =
            query_handler("all_managed_keys", &self.secret_manager_state.query.all_managed_keys)
                .await
                .unwrap()
                .unwrap();

        let managed_key_view = all_managed_keys_view
            .managed_keys
            .values()
            .find(|managed_key_view| {
                managed_key_view.signing_algorithm == Some(signing_algorithm.clone())
                    && managed_key_view.is_signing_key
                    && !managed_key_view.is_removed
            })
            .unwrap();

        let key_id = KeyId::new(&managed_key_view.key_id);

        let public_key = match algorithm {
            Algorithm::ES256 => stronghold_storage.get_es256_public_key(&key_id).await?,
            Algorithm::EdDSA => stronghold_storage.get_ed25519_public_key(&key_id).await?,
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
impl oid4vc_core::Subject for ThisIsTheMainService {
    async fn identifier(&self, subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<String> {
        let did_url: DIDUrl = self
            .key_id(subject_syntax_type, algorithm)
            .await
            .ok_or_else(|| anyhow!("Failed to get key ID for subject syntax type: `{subject_syntax_type}`"))?
            .parse()
            .unwrap();

        Ok(did_url.did().to_string())
    }
}
