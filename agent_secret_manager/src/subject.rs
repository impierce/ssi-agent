use agent_shared::{config::config, from_jsonwebtoken_algorithm_to_jwsalgorithm};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager::{DidMethod, Resolver, SecretManager};
use identity_iota::{did::DID, document::DIDUrlQuery, verification::jwk::JwkParams};
use jsonwebtoken::Algorithm;
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};
use std::sync::Arc;

/// Reponsible for signing and verifying data.
pub struct Subject {
    pub secret_manager: SecretManager,
}

#[async_trait]
impl Verify for Subject {
    async fn public_key(&self, did_url: &str) -> anyhow::Result<Vec<u8>> {
        let did_url = identity_iota::did::DIDUrl::parse(did_url).unwrap();

        let resolver = Resolver::new().await;

        let document = resolver.resolve(did_url.did().as_str()).await.unwrap();

        let verification_method = document
            .resolve_method(
                DIDUrlQuery::from(&did_url),
                Some(identity_iota::verification::MethodScope::VerificationMethod),
            )
            .unwrap();

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
                .ok_or(anyhow::anyhow!("Failed to decode public key for DID URL: {}", did_url))
        })
    }
}

#[async_trait]
impl Sign for Subject {
    async fn key_id(&self, subject_syntax_type: &str, _algorithm: Algorithm) -> Option<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}")).ok()?;

        if method == DidMethod::Web {
            return self
                .secret_manager
                .produce_document(
                    method,
                    Some(did_manager::MethodSpecificParameters::Web { origin: origin() }),
                    from_jsonwebtoken_algorithm_to_jwsalgorithm(
                        &agent_shared::config::get_preferred_signing_algorithm(),
                    ),
                )
                .await
                .ok()
                .and_then(|document| document.verification_method().first().cloned())
                .map(|first| first.id().to_string());
        }

        // TODO: refactor: https://github.com/impierce/ssi-agent/pull/31#discussion_r1634590990

        self.secret_manager
            .produce_document(
                method,
                None,
                from_jsonwebtoken_algorithm_to_jwsalgorithm(&agent_shared::config::get_preferred_signing_algorithm()),
            )
            .await
            .ok()
            .and_then(|document| document.verification_method().first().cloned())
            .map(|first| first.id().to_string())
    }

    async fn sign(&self, message: &str, _subject_syntax_type: &str, _algorithm: Algorithm) -> anyhow::Result<Vec<u8>> {
        Ok(self
            .secret_manager
            .sign(
                message.as_bytes(),
                from_jsonwebtoken_algorithm_to_jwsalgorithm(&agent_shared::config::get_preferred_signing_algorithm()),
            )
            .await?)
    }

    fn external_signer(&self) -> Option<Arc<dyn ExternalSign>> {
        None
    }
}

#[async_trait]
impl oid4vc_core::Subject for Subject {
    async fn identifier(&self, subject_syntax_type: &str, _algorithm: Algorithm) -> anyhow::Result<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}"))?;

        if method == DidMethod::Web {
            return Ok(self
                .secret_manager
                .produce_document(
                    method,
                    Some(did_manager::MethodSpecificParameters::Web { origin: origin() }),
                    from_jsonwebtoken_algorithm_to_jwsalgorithm(
                        &agent_shared::config::get_preferred_signing_algorithm(),
                    ),
                )
                .await
                .map(|document| document.id().to_string())?);
        }

        Ok(self
            .secret_manager
            .produce_document(
                method,
                None,
                from_jsonwebtoken_algorithm_to_jwsalgorithm(&agent_shared::config::get_preferred_signing_algorithm()),
            )
            .await
            .map(|document| document.id().to_string())?)
    }
}

fn origin() -> url::Origin {
    config().url.parse::<url::Url>().unwrap().origin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::config::{set_config, SecretManagerConfig};
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED, ED25519};

    const ES256_SIGNED_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGVXpJMU5pSXNJbU55ZGlJNklsQXRNalUySWl3aWEybGtJam9pTkVGMVdXaFNRMk5HYkc0eWJuUm5VMTlxT1hCRlFtUkxkekl3VUhRdGJHRnFXVWh0V1RkQk1FMUdUU0lzSW10MGVTSTZJa1ZESWl3aWVDSTZJakpNV0dwT1JFOTZWM1J3WlZOWk0ydGlUbEkyWm14YVRVUjRZV2gxYXpKMlVXMWpkWFprUVRodk5EUWlMQ0o1SWpvaVpFRjJSVlpzV0UxSFVFdGFjMnRXV1RSWlZ6QnpPRUk0UzNZM2Myc3hZemt5VDA1WVJFcHZlRjlJY3lKOSMwIn0.eyJpc3MiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaU5FRjFXV2hTUTJOR2JHNHliblJuVTE5cU9YQkZRbVJMZHpJd1VIUXRiR0ZxV1VodFdUZEJNRTFHVFNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SWpKTVdHcE9SRTk2VjNSd1pWTlpNMnRpVGxJMlpteGFUVVI0WVdoMWF6SjJVVzFqZFhaa1FUaHZORFFpTENKNUlqb2laRUYyUlZac1dFMUhVRXRhYzJ0V1dUUlpWekJ6T0VJNFMzWTNjMnN4WXpreVQwNVlSRXB2ZUY5SWN5SjkiLCJzdWIiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaU5FRjFXV2hTUTJOR2JHNHliblJuVTE5cU9YQkZRbVJMZHpJd1VIUXRiR0ZxV1VodFdUZEJNRTFHVFNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SWpKTVdHcE9SRTk2VjNSd1pWTlpNMnRpVGxJMlpteGFUVVI0WVdoMWF6SjJVVzFqZFhaa1FUaHZORFFpTENKNUlqb2laRUYyUlZac1dFMUhVRXRhYzJ0V1dUUlpWekJ6T0VJNFMzWTNjMnN4WXpreVQwNVlSRXB2ZUY5SWN5SjkiLCJhdWQiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlV6STFOaUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaVlrNDNiSEpaWVhOUlZrNDNMVUpZY0MxMFdFVldTR1l0YVhkTWRsVnRiWHByVUZsc2VHWlRWRkZvVlNJc0ltdDBlU0k2SWtWRElpd2llQ0k2SW1odVkyNU5UM2sxU0dGWGJ6SmFTbmhCWW5sWU1GOW1NVTFHU1dsMlRrRmtUMjFXYjNSWGVWZG9ielFpTENKNUlqb2libE5wYkhwMllsTmFYMUp1VWpOU2RreHdkRWxITmpkVWJWVkVhR1ZQWVZGNlltczJhVFJmWDBkeVFTSjkiLCJleHAiOjE3MjMwMjkyMjUsImlhdCI6MTcyMzAyODYyNSwibm9uY2UiOiJ0aGlzIGlzIGEgbm9uY2UifQ.w202CZKOeGM9k35tysJylksBUGI3fvkOgsPPVrfXYZzurns7KF5plMiR_KHH4H_GpYg57Nf2JWa3YEcXGDTVdw";
    const EDDSA_SIGNED_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKSmJWOVpNRkZQTm05SFgyczVNbTlzY1RWTWRIUTJZVkE0YzE5QmJFRmhWVUl6UzBkelVFY3RlR0kwSWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWxaUGFrUjBRblozY0daalNraHlUelpMVjFOUGRYTlZVR1ptUWt3eVIxOUtjWFp0VVRZNFMzaDRWalFpZlEjMCJ9.eyJpc3MiOiJkaWQ6andrOmV5SmhiR2NpT2lKRlpFUlRRU0lzSW1OeWRpSTZJa1ZrTWpVMU1Ua2lMQ0pyYVdRaU9pSkpiVjlaTUZGUE5tOUhYMnM1TW05c2NUVk1kSFEyWVZBNGMxOUJiRUZoVlVJelMwZHpVRWN0ZUdJMElpd2lhM1I1SWpvaVQwdFFJaXdpZUNJNklsWlBha1IwUW5aM2NHWmpTa2h5VHpaTFYxTlBkWE5WVUdabVFrd3lSMTlLY1hadFVUWTRTM2g0VmpRaWZRIiwic3ViIjoiZGlkOmp3azpleUpoYkdjaU9pSkZaRVJUUVNJc0ltTnlkaUk2SWtWa01qVTFNVGtpTENKcmFXUWlPaUpKYlY5Wk1GRlBObTlIWDJzNU1tOXNjVFZNZEhRMllWQTRjMTlCYkVGaFZVSXpTMGR6VUVjdGVHSTBJaXdpYTNSNUlqb2lUMHRRSWl3aWVDSTZJbFpQYWtSMFFuWjNjR1pqU2toeVR6WkxWMU5QZFhOVlVHWm1Ra3d5UjE5S2NYWnRVVFk0UzNoNFZqUWlmUSIsImF1ZCI6ImRpZDpqd2s6ZXlKaGJHY2lPaUpGWkVSVFFTSXNJbU55ZGlJNklrVmtNalUxTVRraUxDSnJhV1FpT2lKdFFqSXhUV2t5Y1V0WVZtTTFOREpVWWt0U09UZ3lUelpUWjFKWVZrWlFaVzV3TTNGWWRIRlRla3R2SWl3aWEzUjVJam9pVDB0UUlpd2llQ0k2SWprM1JVRXpSSE5vUmpONlIwSllTVjlVYnpObVJrUnJNVTFxV1VaYVV6bFZiMUpVYmxCT1NIUlpVV01pZlEiLCJleHAiOjE3MjMwMzE3MTQsImlhdCI6MTcyMzAzMTExNCwibm9uY2UiOiJ0aGlzIGlzIGEgbm9uY2UifQ.oGRYpwH4QvWZs0bZkgAuxq6MqNYdoX44KxNfRl7GzXCnv_0D_c19rhYMwzn04R7udNCthFDr7GUhXLQgROlUDw";

    lazy_static::lazy_static! {
        static ref SECRET_MANAGER_CONFIG: SecretManagerConfig = SecretManagerConfig {
            generate_stronghold: false,
            stronghold_path: "../agent_secret_manager/tests/res/all_slots.stronghold".to_string(),
            stronghold_password: "sup3rSecr3t".to_string(),
            issuer_eddsa_key_id: Some("ed25519-0".to_string()),
            issuer_es256_key_id: Some("es256-0".to_string()),
            issuer_did: None,
            issuer_fragment: None,
        };
    }

    #[tokio::test]
    async fn es256_signed_jwt_successfully_verified() {
        set_config().set_secret_manager_config(SECRET_MANAGER_CONFIG.clone());

        let subject = Arc::new(Subject {
            secret_manager: crate::secret_manager().await,
        });

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

        let subject = Arc::new(Subject {
            secret_manager: crate::secret_manager().await,
        });

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
