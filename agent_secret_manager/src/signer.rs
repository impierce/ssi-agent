use crate::subject::Subject;
use async_trait::async_trait;
use identity_kit::did::DidUrl;
use identity_kit::jose::{JwsAlgorithm, SignError, Signer};
use jsonwebtoken::Algorithm;
use oid4vc_core::Sign;
use std::sync::Arc;

/// Signs for one verification method of the [`Subject`], with that method as `kid`, as IdentityKit's [`Signer`]. It
/// signs with the method's key even when another method is selected to sign in the meantime, so that its signatures
/// always verify with the key their `kid` names.
pub struct SubjectSigner {
    subject: Arc<Subject>,
    method: DidUrl,
    algorithm: Algorithm,
    alg: JwsAlgorithm,
}

impl SubjectSigner {
    /// A signer for the verification method `subject` signs with for `did_method` and `algorithm` now. `None` if
    /// `algorithm` is neither `EdDSA` nor `ES256`, or if `subject` has no such method.
    pub async fn new(subject: Arc<Subject>, did_method: impl Into<String>, algorithm: Algorithm) -> Option<Self> {
        let method = subject.key_id(&did_method.into(), algorithm).await?.parse().ok()?;

        Self::for_method(subject, method, algorithm)
    }

    /// A signer for the verification method `method`, whose key signs with `algorithm`. `None` if `algorithm` is
    /// neither `EdDSA` nor `ES256`.
    pub fn for_method(subject: Arc<Subject>, method: DidUrl, algorithm: Algorithm) -> Option<Self> {
        let alg = match algorithm {
            Algorithm::EdDSA => JwsAlgorithm::EdDSA,
            Algorithm::ES256 => JwsAlgorithm::ES256,
            _ => return None,
        };

        Some(Self {
            subject,
            method,
            algorithm,
            alg,
        })
    }
}

#[async_trait]
impl Signer for SubjectSigner {
    fn alg(&self) -> JwsAlgorithm {
        self.alg
    }

    fn kid(&self) -> Option<&str> {
        Some(self.method.as_str())
    }

    async fn sign(&self, signing_input: &[u8]) -> Result<Vec<u8>, SignError> {
        self.subject
            .sign_for_method(&self.method, self.algorithm, signing_input)
            .await
            .map_err(SignError::msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subject::{MethodKey, StorageKey};
    use agent_shared::config::{get_preferred_did_method, SupportedDidMethod};
    use identity_kit::jose::{encode_compact, DefaultVerifier, JwsHeader, JwsVerifier, KeyType};
    use identity_kit::keys::{InMemoryKeyStore, KeySigner, KeySpec, KeyStore};
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
        let subject = Arc::new(Subject::test_subject().await);
        let store: Arc<dyn KeyStore> = Arc::new(InMemoryKeyStore::new());
        let key = store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();
        let key = KeySigner::new(store, key, JwsAlgorithm::ES256).await.unwrap();
        let method: DidUrl = "did:web:example.org#own-key".parse().unwrap();
        let signing = StorageKey::new(SupportedDidMethod::Web, Algorithm::ES256);
        subject
            .insert_verification_method_id(signing.clone(), method.clone())
            .await
            .unwrap();
        subject
            .set_method_key(method, MethodKey::Available(Box::new(key.clone())))
            .await;
        let signer = SubjectSigner::new(subject.clone(), SupportedDidMethod::Web.to_string(), Algorithm::ES256)
            .await
            .unwrap();
        subject
            .insert_verification_method_id(signing, "did:web:example.org#configured-key".parse().unwrap())
            .await
            .unwrap();

        let signature = signer.sign(b"message").await.unwrap();

        assert!(DefaultVerifier
            .verify(JwsAlgorithm::ES256, b"message", &signature, key.public_jwk())
            .is_ok());
    }

    #[tokio::test]
    async fn unsupported_algorithms_have_no_signer() {
        let subject = Arc::new(Subject::test_subject().await);

        let signer = SubjectSigner::new(subject, get_preferred_did_method().to_string(), Algorithm::RS256).await;

        assert!(signer.is_none());
    }
}
