use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use did_manager::{DidMethod, Resolver, SecretManager};
use futures::executor::block_on;
use identity_iota::{did::DID, document::DIDUrlQuery, verification::jwk::JwkParams};
use oid4vc_core::{authentication::sign::ExternalSign, Sign, Verify};

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
        verification_method
            .data()
            .try_decode()
            .or_else(|_| {
                verification_method
                    .data()
                    .public_key_jwk()
                    .and_then(|public_key_jwk| match public_key_jwk.params() {
                        JwkParams::Okp(okp_params) => Some(okp_params.x.as_bytes().to_vec()),
                        JwkParams::Ec(ec_params) => Some(ec_params.x.as_bytes().to_vec()),
                        _ => None,
                    })
                    .ok_or(anyhow::anyhow!("Failed to decode public key for DID URL: {}", did_url))
            })
            .and_then(|encoded_public_key| URL_SAFE_NO_PAD.decode(encoded_public_key).map_err(Into::into))
    }
}

#[async_trait]
impl Sign for Subject {
    fn key_id(&self, subject_syntax_type: &str) -> Option<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}")).ok()?;

        block_on(async {
            self.secret_manager
                .produce_document(method)
                .await
                .ok()
                .and_then(|document| document.verification_method().first().cloned())
                .map(|first| first.id().to_string())
        })
    }

    fn sign(&self, message: &str, _subject_syntax_type: &str) -> anyhow::Result<Vec<u8>> {
        Ok(block_on(async { self.secret_manager.sign(message.as_bytes()).await })?)
    }

    fn external_signer(&self) -> Option<Arc<dyn ExternalSign>> {
        None
    }
}

#[async_trait]
impl oid4vc_core::Subject for Subject {
    fn identifier(&self, subject_syntax_type: &str) -> anyhow::Result<String> {
        let method: DidMethod = serde_json::from_str(&format!("{subject_syntax_type:?}"))?;

        Ok(block_on(async {
            self.secret_manager
                .produce_document(method)
                .await
                .map(|document| document.id().to_string())
        })?)
    }
}