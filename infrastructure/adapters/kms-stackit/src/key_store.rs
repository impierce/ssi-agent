//! identity-kit's [`KeyStore`] over STACKIT KMS: keys are generated in, and sign inside, a key ring.
//!
//! Deleting a key schedules its deletion after 30 days, as STACKIT does; a deleted key, and a destroyed version, count
//! as missing here.

use crate::api::{Key, KeyState, Version};
use crate::key_name::KeyName;
use crate::provider::{newest_usable, StackitKmsKeyProvider};
use agent_kms::adapter::KeyProviderAdapter as _;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use identity_kit::{
    jose::{ecdsa_signature_from_der, Jwk, JwsAlgorithm, KeyType},
    keys::{KeyId, KeySpec, KeyStore, KeyStoreCapabilities, KeyStoreError},
};

impl StackitKmsKeyProvider {
    /// The key `name` names, unless it is deleted, with the version it pins, which may be destroyed.
    async fn named(&self, name: &KeyId) -> Result<(Key, Option<Version>), KeyStoreError> {
        let missing = || KeyStoreError::NotFound(name.clone());
        let (key, pinned) = match KeyName::parse(name) {
            KeyName::Version { key_id, number } => {
                let key = self.api.key(key_id).await.map_err(KeyStoreError::backend)?;
                let version = self
                    .api
                    .version(key_id, number)
                    .await
                    .map_err(KeyStoreError::backend)?
                    .ok_or_else(missing)?;
                (key, Some(version))
            }
            KeyName::Key(key_id) => (self.api.key(key_id).await.map_err(KeyStoreError::backend)?, None),
            KeyName::Label(label) => {
                let mut labelled = self.labelled(label).await?;
                let key = labelled.next();
                if labelled.next().is_some() {
                    return Err(KeyStoreError::invalid_key(
                        name.clone(),
                        "several keys in the key ring have this label; name one by its key id",
                    ));
                }
                (key, None)
            }
        };
        match key {
            Some(key) if key.state != KeyState::Deleted => Ok((key, pinned)),
            _ => Err(missing()),
        }
    }

    /// The key id and the version `name` signs with: the version it pins, unless that is destroyed, or the key's newest
    /// usable one.
    pub(crate) async fn signing_version(&self, name: &KeyId) -> Result<(String, Version), KeyStoreError> {
        let (key, pinned) = self.named(name).await?;
        if !key.signs_es256() {
            return Err(KeyStoreError::invalid_key(
                name.clone(),
                format!("the key signs with `{}`, not ES256", key.algorithm()),
            ));
        }
        let version = match pinned {
            Some(version) if version.is_destroyed() => return Err(KeyStoreError::NotFound(name.clone())),
            Some(version) => version,
            None => self
                .api
                .versions(&key.id)
                .await
                .map_err(KeyStoreError::backend)?
                .and_then(newest_usable)
                .ok_or_else(|| KeyStoreError::NotFound(name.clone()))?,
        };
        Ok((key.id, version))
    }

    /// Signs with the version `name` pins in one request. Only when that fails are the key and the version looked up,
    /// to tell a deleted key or a destroyed version from a failure.
    async fn sign_pinned(
        &self,
        name: &KeyId,
        key_id: &str,
        number: u64,
        data: &[u8],
    ) -> Result<Vec<u8>, KeyStoreError> {
        match self.api.sign(key_id, number, data).await {
            Ok(Some(signature)) => Ok(signature),
            Ok(None) => Err(KeyStoreError::NotFound(name.clone())),
            Err(error) => match self.signing_version(name).await {
                Err(KeyStoreError::NotFound(missing)) => Err(KeyStoreError::NotFound(missing)),
                _ => Err(KeyStoreError::backend(error)),
            },
        }
    }
}

/// The DER of the PEM-encoded `pem`, such as a public key from STACKIT.
pub(crate) fn pem_der(pem: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .flat_map(|line| line.trim().chars())
        .collect();
    STANDARD.decode(body)
}

#[async_trait]
impl KeyStore for StackitKmsKeyProvider {
    fn capabilities(&self) -> KeyStoreCapabilities {
        KeyStoreCapabilities::new([JwsAlgorithm::ES256])
            .with_generate()
            .with_delete()
            .remote()
    }

    async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
        let (_, version) = self.signing_version(key).await?;
        let pem = version
            .public_key
            .ok_or_else(|| KeyStoreError::invalid_key(key.clone(), "STACKIT KMS returned no public key"))?;
        let der = pem_der(&pem).map_err(|error| KeyStoreError::InvalidKey {
            key: key.clone(),
            reason: "the public key from STACKIT KMS is not PEM".to_string(),
            source: Some(Box::new(error)),
        })?;
        let jwk = Jwk::from_spki_der(KeyType::P256, &der).map_err(|error| KeyStoreError::InvalidKey {
            key: key.clone(),
            reason: "the public key from STACKIT KMS does not decode".to_string(),
            source: Some(Box::new(error)),
        })?;
        Ok(jwk.with_alg(JwsAlgorithm::ES256.as_str()).with_kid(key.as_str()))
    }

    async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
        if alg != JwsAlgorithm::ES256 {
            self.signing_version(key).await?;
            return Err(KeyStoreError::AlgorithmMismatch {
                key: key.clone(),
                key_alg: JwsAlgorithm::ES256,
                requested: alg,
            });
        }
        let der = match KeyName::parse(key) {
            KeyName::Version { key_id, number } => self.sign_pinned(key, key_id, number, data).await?,
            KeyName::Key(_) | KeyName::Label(_) => {
                let (key_id, version) = self.signing_version(key).await?;
                self.api
                    .sign(&key_id, version.number, data)
                    .await
                    .map_err(KeyStoreError::backend)?
                    .ok_or_else(|| KeyStoreError::NotFound(key.clone()))?
            }
        };
        // STACKIT returns ECDSA signatures DER-encoded; JWS wants raw `r || s`.
        ecdsa_signature_from_der(alg, &der).map_err(KeyStoreError::backend)
    }

    async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
        match self.signing_version(key).await {
            Ok(_) => Ok(true),
            Err(KeyStoreError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// A labelled key is named by its label, as the [`KeyStore`] contract asks, any other by `<key id>/<version>`.
    async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        let label = spec.label.clone();
        let key = self.create_key(spec).await?;
        Ok(label.map_or(key, KeyId::new))
    }

    /// Schedules the deletion of the key `key` names, which frees its label. A version it names must exist, but may be
    /// destroyed.
    async fn delete(&self, key: &KeyId) -> Result<(), KeyStoreError> {
        let (named, _) = self.named(key).await?;
        self.api
            .delete_key(&named.id)
            .await
            .map_err(KeyStoreError::backend)?
            .ok_or_else(|| KeyStoreError::NotFound(key.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_stackit::{FakeStackit, ES256};
    use identity_kit::jose::{DefaultVerifier, JwsVerifier as _};
    use identity_kit::keys::testsuite;

    #[tokio::test]
    async fn passes_the_identity_kit_key_store_suite() {
        let stackit = FakeStackit::start().await;

        testsuite::run(&stackit.provider()).await;
    }

    #[test]
    fn reads_the_der_of_a_pem_public_key() {
        let pem = "-----BEGIN PUBLIC KEY-----\nAQID\nBAU=\n-----END PUBLIC KEY-----\n";

        assert_eq!(pem_der(pem).unwrap(), [1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn refuses_a_key_that_does_not_sign_with_es256() {
        let stackit = FakeStackit::start().await;
        stackit.add_key("issuer", "ecdsa_p384_sha384", "active", &[("active", false)]);

        let result = stackit.provider().public_jwk(&KeyId::new("issuer")).await;

        assert!(matches!(result, Err(KeyStoreError::InvalidKey { .. })), "{result:?}");
    }

    #[tokio::test]
    async fn signs_with_a_pinned_version_in_one_request() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_active_key("issuer");

        stackit
            .provider()
            .sign(&KeyId::new(format!("{id}/1")), JwsAlgorithm::ES256, b"message")
            .await
            .unwrap();

        assert_eq!(stackit.kms_requests().await, 1);
    }

    #[tokio::test]
    async fn signs_with_the_newest_usable_version_of_a_key_named_by_its_id() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "active", &[("active", false), ("active", false)]);

        let signature = stackit
            .provider()
            .sign(&KeyId::new(id.as_str()), JwsAlgorithm::ES256, b"message")
            .await
            .unwrap();

        assert!(DefaultVerifier
            .verify(JwsAlgorithm::ES256, b"message", &signature, &stackit.public_jwk(&id, 2))
            .is_ok());
    }

    #[tokio::test]
    async fn reports_a_deleted_key_as_missing_when_signing_with_a_pinned_version() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "deleted", &[("active", false)]);
        let key = KeyId::new(format!("{id}/1"));

        let result = stackit.provider().sign(&key, JwsAlgorithm::ES256, b"message").await;

        assert!(matches!(result, Err(KeyStoreError::NotFound(missing)) if missing == key));
    }

    #[tokio::test]
    async fn reports_a_destroyed_version_as_missing() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "active", &[("destroyed", false)]);
        let key = KeyId::new(format!("{id}/1"));

        let result = stackit.provider().sign(&key, JwsAlgorithm::ES256, b"message").await;

        assert!(matches!(result, Err(KeyStoreError::NotFound(missing)) if missing == key));
    }

    #[tokio::test]
    async fn reports_a_pinned_version_that_cannot_sign_as_a_failure() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "active", &[("active", true)]);

        let result = stackit
            .provider()
            .sign(&KeyId::new(format!("{id}/1")), JwsAlgorithm::ES256, b"message")
            .await;

        assert!(matches!(result, Err(KeyStoreError::Backend(_))), "{result:?}");
    }

    /// A key disabled in STACKIT may be enabled again, so it is retried rather than given up on.
    #[tokio::test]
    async fn keeps_a_disabled_pinned_version_as_existing() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "active", &[("active", true)]);

        let exists = stackit.provider().exists(&KeyId::new(format!("{id}/1"))).await;

        assert!(exists.unwrap());
    }

    #[tokio::test]
    async fn deletes_a_key_whose_pinned_version_is_destroyed() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key("issuer", ES256, "active", &[("destroyed", false)]);

        stackit.provider().delete(&KeyId::new(format!("{id}/1"))).await.unwrap();

        assert_eq!(stackit.key_state(&id), "deleted");
    }

    #[tokio::test]
    async fn does_not_delete_a_key_for_a_version_it_does_not_have() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_active_key("issuer");

        let _ = stackit.provider().delete(&KeyId::new(format!("{id}/9"))).await;

        assert_eq!(stackit.key_state(&id), "active");
    }
}
