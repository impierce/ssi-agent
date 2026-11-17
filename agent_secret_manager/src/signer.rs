use crate::subject::{StorageKey, Subject};
use agent_shared::config::SupportedDidMethod;
use async_trait::async_trait;
use identity_kit::{
    did::DidUrl,
    jose::{JwsAlgorithm, SignError, Signer},
};
use jsonwebtoken::Algorithm;
use std::{str::FromStr, sync::Arc};

/// Signs with the [`Subject`]'s key for one DID method and algorithm, as IdentityKit's [`Signer`].
pub struct SubjectSigner {
    subject: Arc<Subject>,
    algorithm: Algorithm,
    alg: JwsAlgorithm,
    /// The verification method looked up once, so that the key that signs is always the one `kid` names, also when
    /// another key is selected meanwhile.
    verification_method_id: DidUrl,
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
        let did_method = SupportedDidMethod::from_str(&did_method.into()).ok()?;
        let verification_method_id = subject
            .get_verification_method_id(StorageKey::new(did_method, algorithm))
            .await?;

        Some(Self {
            subject,
            algorithm,
            alg,
            kid: verification_method_id.to_string(),
            verification_method_id,
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
        self.subject
            .sign_for_method(&self.verification_method_id, self.algorithm, signing_input)
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
    async fn signs_with_the_key_its_kid_names_after_another_key_is_selected() {
        use crate::subject::MethodKey;
        use identity_kit::{
            jose::JwsVerifier as _,
            keys::{InMemoryKeyStore, KeySpec, KeyStore, KeyType},
        };

        let subject = Arc::new(Subject::test_subject().await);
        let signer = SubjectSigner::new(subject.clone(), "did:key", Algorithm::ES256)
            .await
            .unwrap();
        let key_store = Arc::new(InMemoryKeyStore::new());
        let key_id = key_store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();
        let public_key = key_store.public_jwk(&key_id).await.unwrap();
        let selected: DidUrl = "did:web:example.org#kms".parse().unwrap();
        subject
            .insert_method_key(
                selected.clone(),
                MethodKey {
                    key_store,
                    key_id,
                    public_key,
                },
            )
            .await;
        subject
            .insert_verification_method_ids([(StorageKey::new(SupportedDidMethod::Key, Algorithm::ES256), selected)])
            .await;

        let signature = signer.sign(b"message").await.unwrap();

        let kid_key = subject.resolve_public_key(signer.kid().unwrap()).await.unwrap();
        assert!(identity_kit::jose::DefaultVerifier
            .verify(JwsAlgorithm::ES256, b"message", &signature, &kid_key)
            .is_ok());
    }

    #[tokio::test]
    async fn unsupported_algorithms_have_no_signer() {
        let subject = Arc::new(Subject::test_subject().await);

        let signer = SubjectSigner::new(subject, get_preferred_did_method().to_string(), Algorithm::RS256).await;

        assert!(signer.is_none());
    }
}
