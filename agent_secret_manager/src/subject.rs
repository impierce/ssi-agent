use crate::stronghold_storage;
use agent_shared::config::{
    config, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported, SupportedDidMethod,
};
use anyhow::anyhow;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager_consumer::resolver::Resolver;
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use identity_iota::did::CoreDID;
use identity_iota::storage::JwkStorage;
use identity_iota::{did::DID, document::DIDUrlQuery, verification::jwk::JwkParams};
use itertools::iproduct;
use jsonwebtoken::Algorithm;
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};
use serde_json::json;
use ssi_dids::{DIDMethod, Source};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

// See specification: "Since did:jwk only contains a single key, the DID URL fragment identifier is always a fixed #0 value."
const JWK_FRAGMENT: &str = "0";

/// Reponsible for signing and verifying data.
pub struct Subject {
    pub stronghold_storage: StrongholdExtStorage,
    pub did_methods: Arc<Mutex<DidMethods>>,
}

impl Subject {
    // TODO: For now it is fine that this fail fast (through explicit panics) as it is a critical part of the system. In
    // the future this functionality should be implemented as an actual Domain (through the cqrs-es framework).
    // Create a new Subject.
    pub async fn new() -> Self {
        let stronghold_storage = stronghold_storage().await;
        let mut did_methods = DidMethods::default();

        let signing_algorithms = get_all_enabled_signing_algorithms_supported();
        let non_updateable_did_methods = get_all_enabled_did_methods()
            .clone()
            .into_iter()
            .filter(|method| !method.is_updateable())
            .collect::<Vec<_>>();

        let cartesian_product = iproduct!(non_updateable_did_methods.into_iter(), signing_algorithms.into_iter())
            .map(|(did_method, signing_algorithm)| (did_method, signing_algorithm))
            .collect::<Vec<_>>();

        let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();
        let es256_key_id = config().secret_manager.issuer_es256_key_id.clone();

        for (did_method, signing_algorithm) in cartesian_product {
            let public_key_jwk = match signing_algorithm {
                Algorithm::EdDSA => {
                    let public_key_jwk = json!(stronghold_storage
                        .get_ed25519_public_key(&ed25519_key_id)
                        .await
                        .expect("Could not find EdDSA public key"));

                    public_key_jwk
                }
                Algorithm::ES256 => {
                    let public_key_jwk = json!(stronghold_storage
                        .get_es256_public_key(&es256_key_id)
                        .await
                        .expect("Could not find ES256 public key"));

                    public_key_jwk
                }
                _ => {
                    panic!("Unsuported algorithm");
                }
            };

            let jwk: ssi_jwk::JWK = serde_json::from_value(public_key_jwk.clone()).unwrap();

            let (controller, verification_method_id) = match did_method {
                SupportedDidMethod::Jwk => {
                    let controller =
                        CoreDID::parse(did_jwk_extern::DIDJWK.generate(&Source::Key(&jwk)).unwrap()).unwrap();
                    let verification_method_id = format!("{controller}#{JWK_FRAGMENT}");

                    (controller, verification_method_id)
                }
                SupportedDidMethod::Key => {
                    let controller =
                        CoreDID::parse(did_key_extern::DIDKey.generate(&Source::Key(&jwk)).unwrap()).unwrap();
                    let verification_method_id = format!("{controller}#{}", controller.method_id());

                    (controller, verification_method_id)
                }
                _ => {
                    panic!("Updateable DID method");
                }
            };

            did_methods.insert_did(&did_method, signing_algorithm, controller.to_string());
            did_methods.insert_verification_method_id(&did_method, signing_algorithm, &verification_method_id);
        }

        Self {
            stronghold_storage,
            did_methods: Arc::new(Mutex::new(did_methods)),
        }
    }
}

#[async_trait]
impl Verify for Subject {
    async fn public_key(&self, did_url: &str) -> anyhow::Result<Vec<u8>> {
        let did_url =
            identity_iota::did::DIDUrl::parse(did_url).map_err(|err| anyhow!("Failed to parse DID URL: {err}"))?;

        let resolver = Resolver::new().await;

        let document = resolver
            .resolve(did_url.did().as_str())
            .await
            .map_err(|err| anyhow!("Failed to resolve DID Document for DID: {did_url}, error: {err}"))?;

        let verification_method = document
            .resolve_method(
                DIDUrlQuery::from(&did_url),
                Some(identity_iota::verification::MethodScope::VerificationMethod),
            )
            .ok_or(anyhow!("Failed to resolve verification method for DID URL: {did_url}"))?;

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
                .ok_or(anyhow!("Failed to decode public key for DID URL: {}", did_url))
        })
    }
}

#[async_trait]
impl Sign for Subject {
    async fn key_id(&self, subject_syntax_type: &str, algorithm: Algorithm) -> Option<String> {
        let method = SupportedDidMethod::from_str(subject_syntax_type).ok()?;

        self.did_methods
            .lock()
            .await
            .get(&method)
            .get(&algorithm)
            .and_then(|document_data| document_data.verification_method_id.clone())
    }

    async fn sign(&self, message: &str, _subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<Vec<u8>> {
        let (key_id, public_key) = match algorithm {
            Algorithm::ES256 => {
                let es256_key_id = config().secret_manager.issuer_es256_key_id.clone();
                let public_key = self.stronghold_storage.get_es256_public_key(&es256_key_id).await?;
                (es256_key_id, public_key)
            }
            Algorithm::EdDSA => {
                let ed25519_key_id = config().secret_manager.issuer_eddsa_key_id.clone();
                let public_key = self.stronghold_storage.get_ed25519_public_key(&ed25519_key_id).await?;
                (ed25519_key_id, public_key)
            }
            _ => return Err(anyhow!("Unsupported algorithm")),
        };

        let signature = self
            .stronghold_storage
            .sign(&key_id, message.as_bytes(), &public_key)
            .await?;

        Ok(signature)
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

        let did = self
            .did_methods
            .lock()
            .await
            .get(&method)
            .get(&algorithm)
            .ok_or(anyhow!("Failed to get DID for method: {method}"))?
            .did
            .clone();

        Ok(did)
    }
}

/// Stores all the DIDs and their associated Verification Method IDs for each (enabled) DID method.
#[derive(Default, Debug)]
pub struct DidMethods {
    pub did_iota: Algorithms,
    pub did_iota_smr: Algorithms,
    pub did_iota_rms: Algorithms,
    pub did_jwk: Algorithms,
    pub did_key: Algorithms,
    pub did_web: Algorithms,
}

impl DidMethods {
    pub fn get(&self, method: &SupportedDidMethod) -> &Algorithms {
        match method {
            SupportedDidMethod::Iota => &self.did_iota,
            SupportedDidMethod::IotaSmr => &self.did_iota_smr,
            SupportedDidMethod::IotaRms => &self.did_iota_rms,
            SupportedDidMethod::Jwk => &self.did_jwk,
            SupportedDidMethod::Key => &self.did_key,
            SupportedDidMethod::Web => &self.did_web,
        }
    }

    pub fn insert_did(&mut self, method: &SupportedDidMethod, algorithm: Algorithm, did: String) {
        let algorithms = match method {
            SupportedDidMethod::Iota => &mut self.did_iota,
            SupportedDidMethod::IotaSmr => &mut self.did_iota_smr,
            SupportedDidMethod::IotaRms => &mut self.did_iota_rms,
            SupportedDidMethod::Jwk => &mut self.did_jwk,
            SupportedDidMethod::Key => &mut self.did_key,
            SupportedDidMethod::Web => &mut self.did_web,
        };

        match algorithm {
            Algorithm::ES256 => {
                let _ = algorithms.es256.insert(DocumentData {
                    did,
                    verification_method_id: None,
                });
            }
            Algorithm::EdDSA => {
                let _ = algorithms.eddsa.insert(DocumentData {
                    did,
                    verification_method_id: None,
                });
            }
            _ => {}
        }
    }

    pub fn insert_verification_method_id(
        &mut self,
        method: &SupportedDidMethod,
        algorithm: Algorithm,
        verification_method_id: &str,
    ) {
        let algorithms = match method {
            SupportedDidMethod::Iota => &mut self.did_iota,
            SupportedDidMethod::IotaSmr => &mut self.did_iota_smr,
            SupportedDidMethod::IotaRms => &mut self.did_iota_rms,
            SupportedDidMethod::Jwk => &mut self.did_jwk,
            SupportedDidMethod::Key => &mut self.did_key,
            SupportedDidMethod::Web => &mut self.did_web,
        };

        match algorithm {
            Algorithm::ES256 => {
                if let Some(document_data) = &mut algorithms.es256 {
                    document_data.verification_method_id = Some(verification_method_id.to_string());
                }
            }
            Algorithm::EdDSA => {
                if let Some(document_data) = &mut algorithms.eddsa {
                    document_data.verification_method_id = Some(verification_method_id.to_string());
                }
            }
            _ => {}
        }
    }
}

#[derive(Default, Debug)]
pub struct Algorithms {
    pub es256: Option<DocumentData>,
    pub eddsa: Option<DocumentData>,
}

impl Algorithms {
    pub fn get(&self, algorithm: &Algorithm) -> Option<&DocumentData> {
        match algorithm {
            Algorithm::ES256 => self.es256.as_ref(),
            Algorithm::EdDSA => self.eddsa.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct DocumentData {
    pub did: String,
    pub verification_method_id: Option<String>,
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

        let subject = Arc::new(Subject::new().await);

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

        let subject = Arc::new(Subject::new().await);

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
}
