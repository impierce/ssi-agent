//! identity-kit's [`KeyStore`] over AWS KMS: keys are generated in, and sign inside, AWS KMS.
//!
//! A key is named by its ARN, its key ID or an alias (`alias/<name>`). A key generated with a label gets the alias
//! `alias/<label>` and is named by the label, as the [`KeyStore`] contract asks. AWS KMS never deletes a key at
//! once but schedules its deletion; a key pending deletion counts as deleted here, and can still be restored in
//! AWS KMS until the waiting period ends.

use crate::{describe, AwsKmsAmbientKeyProvider};
use async_trait::async_trait;
use aws_sdk_kms::{
    error::{ProvideErrorMetadata, SdkError},
    primitives::Blob,
    types::{KeyMetadata, KeySpec as AwsKeySpec, KeyState, KeyUsageType, MessageType, SigningAlgorithmSpec, Tag},
};
use identity_kit::{
    jose::{ecdsa_signature_from_der, Jwk, JwsAlgorithm, KeyType},
    keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError},
};
use sha2::{Digest as _, Sha256};
use std::future::Future;
use thiserror::Error;

/// The algorithms the store signs with.
const ALGORITHMS: [JwsAlgorithm; 2] = [JwsAlgorithm::ES256, JwsAlgorithm::EdDSA];
/// AWS KMS signs at most this many bytes of a message it is given whole, which Ed25519 requires.
const MAX_RAW_MESSAGE: usize = 4096;

/// An error reported by AWS KMS, or a request it would refuse.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct AwsKmsError(String);

impl AwsKmsAmbientKeyProvider {
    /// Runs `request` within the deadline, reporting a timeout as a backend error.
    async fn bounded<T>(&self, request: impl Future<Output = Result<T, KeyStoreError>>) -> Result<T, KeyStoreError> {
        tokio::time::timeout(self.deadline, Box::pin(request))
            .await
            .unwrap_or_else(|_| {
                Err(KeyStoreError::backend(AwsKmsError(format!(
                    "AWS KMS did not respond within {} seconds",
                    self.deadline.as_secs_f32()
                ))))
            })
    }

    /// The key's metadata, or `None` when it does not exist or is pending deletion.
    async fn metadata(&self, key: &KeyId) -> Result<Option<KeyMetadata>, KeyStoreError> {
        match self.client().await.describe_key().key_id(aws_key_id(key)).send().await {
            Ok(output) => Ok(output
                .key_metadata
                .filter(|metadata| metadata.key_state() != Some(&KeyState::PendingDeletion))),
            Err(error) if error.code() == Some("NotFoundException") => Ok(None),
            Err(error) => Err(backend(&error)),
        }
    }

    /// The algorithm `key` signs with. A key's spec never changes in AWS KMS, so it is looked up once.
    async fn algorithm(&self, key: &KeyId) -> Result<JwsAlgorithm, KeyStoreError> {
        if let Some(alg) = self.algorithms.read().expect("lock poisoned").get(key.as_str()) {
            return Ok(*alg);
        }
        let metadata = self
            .metadata(key)
            .await?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
        let alg = signing_algorithm(&metadata).ok_or_else(|| {
            KeyStoreError::invalid_key(
                key.clone(),
                format!(
                    "AWS KMS key spec {:?} with usage {:?} does not sign with a supported algorithm",
                    metadata.key_spec(),
                    metadata.key_usage()
                ),
            )
        })?;
        self.algorithms
            .write()
            .expect("lock poisoned")
            .insert(key.as_str().to_string(), alg);
        Ok(alg)
    }

    /// Maps an error of an operation on `key`: an unknown key, or one pending deletion, is `NotFound`.
    async fn key_error<E, R>(&self, key: &KeyId, error: &SdkError<E, R>) -> KeyStoreError
    where
        E: ProvideErrorMetadata + std::error::Error + 'static,
        R: std::fmt::Debug,
    {
        match error.code() {
            Some("NotFoundException") => KeyStoreError::NotFound(key.clone()),
            // AWS KMS reports a key pending deletion, but also one still being imported, as in an invalid state.
            Some("KMSInvalidStateException") if matches!(self.metadata(key).await, Ok(None)) => {
                KeyStoreError::NotFound(key.clone())
            }
            _ => backend(error),
        }
    }
}

#[async_trait]
impl KeyStore for AwsKmsAmbientKeyProvider {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new(ALGORITHMS)
            .with_generate()
            .with_delete()
            .remote()
    }

    async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
        self.bounded(async {
            let alg = self.algorithm(key).await?;
            let output = match self
                .client()
                .await
                .get_public_key()
                .key_id(aws_key_id(key))
                .send()
                .await
            {
                Ok(output) => output,
                Err(error) => return Err(self.key_error(key, &error).await),
            };
            let der = output
                .public_key()
                .ok_or_else(|| KeyStoreError::invalid_key(key.clone(), "AWS KMS returned no public key"))?;
            let jwk = Jwk::from_spki_der(key_type(alg), der.as_ref()).map_err(|error| KeyStoreError::InvalidKey {
                key: key.clone(),
                reason: "the public key from AWS KMS does not decode".to_string(),
                source: Some(Box::new(error)),
            })?;
            Ok(jwk.with_alg(alg.as_str()).with_kid(key.as_str()))
        })
        .await
    }

    async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        self.bounded(async {
            let key_alg = self.algorithm(key).await?;
            if key_alg != alg {
                return Err(KeyStoreError::AlgorithmMismatch {
                    key: key.clone(),
                    key_alg,
                    requested: alg,
                });
            }

            // ES256 signs a digest, so any size works. Ed25519 signs the whole message, and AWS KMS accepts at most
            // 4096 bytes of it; its prehashed variant (Ed25519ph) is a different algorithm from JOSE's EdDSA.
            let (message, message_type, algorithm) = match alg {
                JwsAlgorithm::ES256 => (
                    Sha256::digest(data).to_vec(),
                    MessageType::Digest,
                    SigningAlgorithmSpec::EcdsaSha256,
                ),
                JwsAlgorithm::EdDSA if data.len() > MAX_RAW_MESSAGE => {
                    return Err(KeyStoreError::backend(AwsKmsError(format!(
                        "AWS KMS signs at most {MAX_RAW_MESSAGE} bytes with Ed25519; this signing input has {} bytes",
                        data.len()
                    ))));
                }
                JwsAlgorithm::EdDSA => (data.to_vec(), MessageType::Raw, SigningAlgorithmSpec::Ed25519Sha512),
                alg => return Err(KeyStoreError::UnsupportedAlgorithm(alg)),
            };

            let output = match self
                .client()
                .await
                .sign()
                .key_id(aws_key_id(key))
                .message(Blob::new(message))
                .message_type(message_type)
                .signing_algorithm(algorithm)
                .send()
                .await
            {
                Ok(output) => output,
                Err(error) => return Err(self.key_error(key, &error).await),
            };
            let signature = output
                .signature()
                .ok_or_else(|| KeyStoreError::backend(AwsKmsError("AWS KMS returned no signature".to_string())))?;

            match alg {
                // AWS KMS returns ECDSA signatures DER-encoded; JWS wants raw `r || s`.
                JwsAlgorithm::ES256 => {
                    ecdsa_signature_from_der(alg, signature.as_ref()).map_err(KeyStoreError::backend)
                }
                _ => Ok(signature.as_ref().to_vec()),
            }
        })
        .await
    }

    async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        self.bounded(async { Ok(self.metadata(key).await?.is_some()) }).await
    }

    async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        spec.validate()?;
        let aws_spec = match spec.key_type {
            KeyType::P256 => AwsKeySpec::EccNistP256,
            KeyType::Ed25519 => AwsKeySpec::EccNistEdwards25519,
            key_type => return Err(KeyStoreError::UnsupportedKeyType(key_type)),
        };
        if !ALGORITHMS.contains(&spec.alg) {
            return Err(KeyStoreError::UnsupportedAlgorithm(spec.alg));
        }

        self.bounded(async {
            let label = spec.label.as_deref().map(alias_for_label).transpose()?;
            if let Some((label, alias)) = &label {
                if self.metadata(&KeyId::new(alias.as_str())).await?.is_some() {
                    return Err(KeyStoreError::AlreadyExists(KeyId::new(label.as_str())));
                }
            }

            let tags: Vec<Tag> = self
                .key_tags
                .iter()
                .map(|(key, value)| Tag::builder().tag_key(key).tag_value(value).build())
                .collect::<Result<_, _>>()
                .map_err(KeyStoreError::backend)?;
            let created = self
                .client()
                .await
                .create_key()
                .key_usage(KeyUsageType::SignVerify)
                .key_spec(aws_spec)
                .set_tags((!tags.is_empty()).then_some(tags))
                .send()
                .await
                .map_err(|error| backend(&error))?;
            let metadata = created
                .key_metadata()
                .ok_or_else(|| KeyStoreError::backend(AwsKmsError("AWS KMS returned no key metadata".to_string())))?;
            let key_id = metadata.key_id().to_string();

            let id = match label {
                Some((label, alias)) => {
                    if let Err(error) = self
                        .client()
                        .await
                        .create_alias()
                        .alias_name(&alias)
                        .target_key_id(&key_id)
                        .send()
                        .await
                    {
                        // Don't leave a key behind that nothing names.
                        let _ = self.client().await.schedule_key_deletion().key_id(&key_id).send().await;
                        return Err(match error.code() {
                            Some("AlreadyExistsException") => KeyStoreError::AlreadyExists(KeyId::new(label)),
                            _ => backend(&error),
                        });
                    }
                    KeyId::new(label)
                }
                None => KeyId::new(metadata.arn().unwrap_or(&key_id)),
            };

            self.algorithms
                .write()
                .expect("lock poisoned")
                .insert(id.as_str().to_string(), spec.alg);
            Ok(id)
        })
        .await
    }

    async fn delete(&self, key: &KeyId) -> Result<(), KeyStoreError> {
        self.bounded(async {
            let metadata = self
                .metadata(key)
                .await?
                .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?;
            self.client()
                .await
                .schedule_key_deletion()
                .key_id(metadata.key_id())
                .send()
                .await
                .map_err(|error| backend(&error))?;

            // Free the label, so it names nothing and can be used again.
            let aws_id = aws_key_id(key);
            if aws_id.starts_with("alias/") {
                match self.client().await.delete_alias().alias_name(&aws_id).send().await {
                    Err(error) if error.code() != Some("NotFoundException") => return Err(backend(&error)),
                    _ => {}
                }
            }

            self.algorithms.write().expect("lock poisoned").remove(key.as_str());
            Ok(())
        })
        .await
    }
}

/// How AWS KMS names `key`: ARNs, key IDs and aliases as they are; any other id is a label, named by its alias.
fn aws_key_id(key: &KeyId) -> String {
    let id = key.as_str();
    if id.starts_with("arn:") || id.starts_with("alias/") || id.starts_with("mrk-") || is_key_uuid(id) {
        id.to_string()
    } else {
        format!("alias/{id}")
    }
}

/// The label and alias for a key generated with `label`, if AWS KMS accepts it as an alias name.
fn alias_for_label(label: &str) -> Result<(String, String), KeyStoreError> {
    let valid = !label.is_empty()
        && label.len() <= 250
        && !label.starts_with("aws/")
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-'));
    let alias = aws_key_id(&KeyId::new(label));
    // An id AWS KMS reads as an ARN, key ID or alias is not a label.
    if !valid || alias == label {
        return Err(KeyStoreError::backend(AwsKmsError(format!(
            "`{label}` can't be an AWS KMS alias name: use letters, digits, `/`, `_` and `-`, not an ARN or key ID"
        ))));
    }
    Ok((label.to_string(), alias))
}

fn is_key_uuid(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// The JWS algorithm a key signs with, if this store supports it.
fn signing_algorithm(metadata: &KeyMetadata) -> Option<JwsAlgorithm> {
    if metadata.key_usage() != Some(&KeyUsageType::SignVerify) {
        return None;
    }
    match metadata.key_spec()? {
        AwsKeySpec::EccNistP256 => Some(JwsAlgorithm::ES256),
        AwsKeySpec::EccNistEdwards25519 => Some(JwsAlgorithm::EdDSA),
        _ => None,
    }
}

fn key_type(alg: JwsAlgorithm) -> KeyType {
    match alg {
        JwsAlgorithm::EdDSA => KeyType::Ed25519,
        _ => KeyType::P256,
    }
}

fn backend<E, R>(error: &SdkError<E, R>) -> KeyStoreError
where
    E: ProvideErrorMetadata + std::error::Error + 'static,
    R: std::fmt::Debug,
{
    KeyStoreError::backend(AwsKmsError(describe(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_kms::FakeKms;
    use identity_kit::keys::testsuite;

    #[tokio::test]
    async fn passes_the_identity_kit_key_store_suite() {
        let kms = FakeKms::start().await;

        testsuite::run(&kms.provider()).await;
    }

    #[tokio::test]
    async fn signs_es256_input_larger_than_aws_accepts_whole() {
        let kms = FakeKms::start().await;
        let store = kms.provider();
        let key = store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();

        let signature = store.sign(&key, JwsAlgorithm::ES256, &[7; 10_000]).await.unwrap();

        assert_eq!(signature.len(), 64);
    }

    #[tokio::test]
    async fn refuses_ed25519_input_larger_than_aws_accepts() {
        let kms = FakeKms::start().await;
        let store = kms.provider();
        let key = store
            .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA))
            .await
            .unwrap();

        let error = store.sign(&key, JwsAlgorithm::EdDSA, &[7; 5_000]).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "key store backend error: AWS KMS signs at most 4096 bytes with Ed25519; this signing input has 5000 bytes"
        );
    }

    #[tokio::test]
    async fn deletes_the_new_key_when_its_label_cannot_be_given() {
        let kms = FakeKms::start().await;
        kms.refuse_aliases();

        let _ = kms
            .provider()
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("issuer"))
            .await;

        assert_eq!(kms.key_states(), ["PendingDeletion"]);
    }

    #[tokio::test]
    async fn reports_a_key_deleted_outside_the_store_as_not_found() {
        let kms = FakeKms::start().await;
        let store = kms.provider();
        let key = store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();
        kms.set_key_state(key.as_str(), "PendingDeletion");

        let error = store.sign(&key, JwsAlgorithm::ES256, b"data").await.unwrap_err();

        assert_eq!(error.to_string(), format!("key `{key}` not found"));
    }

    #[tokio::test]
    async fn reports_a_disabled_key_as_a_backend_error() {
        let kms = FakeKms::start().await;
        let store = kms.provider();
        let key = store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();
        kms.set_key_state(key.as_str(), "Disabled");

        let error = store.sign(&key, JwsAlgorithm::ES256, b"data").await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "key store backend error: DisabledException: rejected by the fake"
        );
    }

    #[tokio::test]
    async fn refuses_a_key_of_a_type_it_does_not_sign_with() {
        let kms = FakeKms::start().await;
        let key = KeyId::new(kms.add_rsa_key());

        let error = kms.provider().public_jwk(&key).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "key `{key}` is unusable: AWS KMS key spec Some(Rsa2048) with usage Some(SignVerify) does not sign \
                 with a supported algorithm"
            )
        );
    }

    #[test]
    fn names_a_label_by_its_alias() {
        assert_eq!(aws_key_id(&KeyId::new("issuer")), "alias/issuer");
    }

    #[test]
    fn passes_a_key_id_to_aws_as_it_is() {
        let key_id = "1234abcd-12ab-34cd-56ef-1234567890ab";

        assert_eq!(aws_key_id(&KeyId::new(key_id)), key_id);
    }

    #[test]
    fn passes_an_alias_to_aws_as_it_is() {
        assert_eq!(aws_key_id(&KeyId::new("alias/issuer")), "alias/issuer");
    }

    #[test]
    fn refuses_a_key_id_as_a_label() {
        let error = alias_for_label("1234abcd-12ab-34cd-56ef-1234567890ab").unwrap_err();

        assert_eq!(
            error.to_string(),
            "key store backend error: `1234abcd-12ab-34cd-56ef-1234567890ab` can't be an AWS KMS alias name: use \
             letters, digits, `/`, `_` and `-`, not an ARN or key ID"
        );
    }

    #[test]
    fn refuses_an_alias_aws_reserves_as_a_label() {
        let error = alias_for_label("aws/reserved").unwrap_err();

        assert_eq!(
            error.to_string(),
            "key store backend error: `aws/reserved` can't be an AWS KMS alias name: use letters, digits, `/`, `_` \
             and `-`, not an ARN or key ID"
        );
    }
}
