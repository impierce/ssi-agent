use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager::{DidMethod, Resolver, SecretManager};
use identity_iota::{core::ToJson, did::DID, document::DIDUrlQuery, verification::jwk::JwkParams};
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
                        // FIX THIS: Error handling
                        let x_bytes = URL_SAFE_NO_PAD.decode(&ec_params.x).unwrap();
                        let y_bytes = URL_SAFE_NO_PAD.decode(&ec_params.y).unwrap();

                        let encoded_point = p256::EncodedPoint::from_affine_coordinates(
                            &p256::FieldBytes::from_slice(&x_bytes),
                            &p256::FieldBytes::from_slice(&y_bytes),
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
        // .and_then(|encoded_public_key| {
        //     URL_SAFE_NO_PAD
        //         .decode(encoded_public_key)
        //         .map_err(Into::into)
        //         .inspect_err(|e| log::info!("Failed to decode public key: {}", e))
        // })
    }
}

#[async_trait]
impl Sign for Subject {
    async fn key_id(&self, subject_syntax_type: &str, algorithm: Algorithm) -> Option<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}")).ok()?;

        self.secret_manager
            .produce_document(method, serde_json::from_str(&algorithm.to_json().unwrap()).unwrap())
            .await
            .ok()
            .and_then(|document| document.verification_method().first().cloned())
            .map(|first| first.id().to_string())
    }

    async fn sign(&self, message: &str, _subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<Vec<u8>> {
        Ok(self
            .secret_manager
            .sign(
                message.as_bytes(),
                serde_json::from_str(&algorithm.to_json().unwrap()).unwrap(),
            )
            .await?)
    }

    fn external_signer(&self) -> Option<Arc<dyn ExternalSign>> {
        None
    }
}

#[async_trait]
impl oid4vc_core::Subject for Subject {
    async fn identifier(&self, subject_syntax_type: &str, algorithm: Algorithm) -> anyhow::Result<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}"))?;

        Ok(self
            .secret_manager
            .produce_document(method, serde_json::from_str(&algorithm.to_json().unwrap()).unwrap())
            .await
            .map(|document| document.id().to_string())?)
    }
}
