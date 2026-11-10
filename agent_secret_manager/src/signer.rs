use crate::subject::Subject;
use async_trait::async_trait;
use identity_kit::jose::{JwsAlgorithm, SignError, Signer};
use jsonwebtoken::Algorithm;
use oid4vc_core::Sign;
use std::sync::Arc;

/// Signs with the [`Subject`]'s key for one DID method and algorithm, as IdentityKit's [`Signer`].
pub struct SubjectSigner {
    subject: Arc<Subject>,
    did_method: String,
    algorithm: Algorithm,
    alg: JwsAlgorithm,
    kid: String,
}

impl SubjectSigner {
    /// A signer for the key `subject` holds for `did_method` and `algorithm`, with the key's verification method as
    /// `kid`. `None` if `algorithm` is neither `EdDSA` nor `ES256`, or if `subject` has no such key.
    pub async fn new(subject: Arc<Subject>, did_method: impl Into<String>, algorithm: Algorithm) -> Option<Self> {
        let alg = match algorithm {
            Algorithm::EdDSA => JwsAlgorithm::EdDSA,
            Algorithm::ES256 => JwsAlgorithm::ES256,
            _ => return None,
        };
        let did_method = did_method.into();
        let kid = subject.key_id(&did_method, algorithm).await?;

        Some(Self {
            subject,
            did_method,
            algorithm,
            alg,
            kid,
        })
    }
}

#[async_trait]
impl Signer for SubjectSigner {
    fn alg(&self) -> JwsAlgorithm {
        self.alg
    }

    fn kid(&self) -> Option<&str> {
        Some(&self.kid)
    }

    async fn sign(&self, signing_input: &[u8]) -> Result<Vec<u8>, SignError> {
        let message = std::str::from_utf8(signing_input).map_err(SignError::new)?;

        Sign::sign(self.subject.as_ref(), message, &self.did_method, self.algorithm)
            .await
            .map_err(SignError::msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::config::get_preferred_did_method;
    use identity_kit::jose::{encode_compact, JwsHeader};
    use jsonwebtoken::{DecodingKey, Validation};
    use rstest::rstest;
    use serde_json::json;

    #[rstest]
    #[case::es256(Algorithm::ES256)]
    #[case::eddsa(Algorithm::EdDSA)]
    #[tokio::test]
    async fn signs_jws_verifiable_with_the_resolved_key(#[case] algorithm: Algorithm) {
        let subject = Arc::new(Subject::test_subject().await);
        let signer = SubjectSigner::new(subject.clone(), get_preferred_did_method().to_string(), algorithm)
            .await
            .unwrap();
        let kid = signer.kid().unwrap().to_string();

        let header = JwsHeader::new(signer.alg()).with_typ("JWT").with_kid(&kid);
        let payload = json!({
            "iss": "test-issuer",
            "sub": "test-subject",
            "iat": 1234567890
        });
        let jwt = encode_compact(&header, &serde_json::to_vec(&payload).unwrap(), &signer)
            .await
            .unwrap();

        let public_key = subject.resolve_public_key(&kid).await.unwrap();
        let decoding_key = DecodingKey::from_jwk(&serde_json::from_value(json!(public_key)).unwrap()).unwrap();
        let mut validation = Validation::new(algorithm);
        validation.required_spec_claims = Default::default();

        let token = jsonwebtoken::decode::<serde_json::Value>(&jwt, &decoding_key, &validation).unwrap();

        assert_eq!(token.header.kid, Some(kid));
        assert_eq!(token.claims, payload);
    }

    #[tokio::test]
    async fn unsupported_algorithms_have_no_signer() {
        let subject = Arc::new(Subject::test_subject().await);

        let signer = SubjectSigner::new(subject, get_preferred_did_method().to_string(), Algorithm::RS256).await;

        assert!(signer.is_none());
    }
}
