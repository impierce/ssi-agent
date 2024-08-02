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
