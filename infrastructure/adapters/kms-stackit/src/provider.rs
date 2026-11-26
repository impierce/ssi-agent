//! The STACKIT KMS key provider for one key ring, and the factory that builds one from a provider's settings.

use crate::api::{Key, KeyState, KmsApi, StackitKmsError, Version, KMS_URL};
use crate::key_name::{label_refusal, KeyName};
use crate::sign_in::{AccessTokens, SignIn};
use agent_kms::adapter::{
    ConnectionError, KeyProviderAdapter, KeyProviderFactory, KeyRefusal, ProviderKey, ProviderKeyState, ProviderKeys,
};
use agent_kms::key_provider::settings::{KeyProviderSettings, StackitKmsSettings};
use async_trait::async_trait;
use identity_kit::{
    jose::JwsAlgorithm,
    keys::{KeyId, KeySpec, KeyStoreError},
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

/// How long a new key may take to get a version that can sign.
const VERSION_WAIT: Duration = Duration::from_secs(30);
/// How often a new key's versions are asked for while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// STACKIT KMS, for the keys of one key ring. Keys are P-256 and sign with ES256, with `software` protection.
///
/// A key created through it is named `<key id>/<version>`, which pins one version; a key id alone, or a label, names
/// the key's newest version that can sign. A label is the key's display name, which STACKIT doesn't keep unique, so a
/// label is only given when no key in the key ring that isn't deleted has it.
#[derive(Clone)]
pub struct StackitKmsKeyProvider {
    pub(crate) api: Arc<KmsApi>,
    /// Held while a labelled key is created, so that two creates on this instance can't both take one label.
    labels: Arc<Mutex<()>>,
    pub(crate) version_wait: Duration,
    pub(crate) poll_interval: Duration,
}

impl StackitKmsKeyProvider {
    /// The provider for the key ring of `settings` in STACKIT KMS at `kms_url`, signed in to with `sign_in`.
    ///
    /// # Errors
    ///
    /// When no HTTP client can be set up.
    pub fn new(kms_url: &Url, settings: &StackitKmsSettings, sign_in: SignIn) -> Result<Self, reqwest::Error> {
        let http = KmsApi::client()?;
        let tokens = AccessTokens::new(sign_in, http.clone());
        Ok(Self {
            api: Arc::new(KmsApi::new(kms_url, settings, tokens, http)),
            labels: Arc::default(),
            version_wait: VERSION_WAIT,
            poll_interval: POLL_INTERVAL,
        })
    }

    /// Creates a key named `label`, or a generated name, and waits for a version that can sign. A key that gets none
    /// is deleted again, which frees its label.
    async fn create(&self, label: Option<String>) -> Result<KeyId, KeyStoreError> {
        let _label_lock = match &label {
            Some(_) => Some(self.labels.lock().await),
            None => None,
        };
        if let Some(label) = &label {
            if self.labelled(label).await?.next().is_some() {
                return Err(KeyStoreError::AlreadyExists(KeyId::new(label.as_str())));
            }
        }

        let display_name = label.unwrap_or_else(|| format!("unicore-{}", Uuid::new_v4()));
        let key = self
            .api
            .create_key(&display_name)
            .await
            .map_err(KeyStoreError::backend)?;
        match self.first_usable_version(&key.id).await {
            Ok(version) => Ok(KeyId::new(format!("{}/{}", key.id, version.number))),
            Err(cause) => match self.api.delete_key(&key.id).await {
                Ok(_) => Err(KeyStoreError::backend(cause)),
                Err(deletion) => Err(KeyStoreError::backend(StackitKmsError::Abandoned {
                    key_id: key.id,
                    cause: Box::new(cause),
                    deletion: Box::new(deletion),
                })),
            },
        }
    }

    /// The newest usable version of the new key `key_id`, once it has one.
    async fn first_usable_version(&self, key_id: &str) -> Result<Version, StackitKmsError> {
        let polling = async {
            loop {
                let versions = self.api.versions(key_id).await?.unwrap_or_default();
                if let Some(version) = newest_usable(versions) {
                    return Ok(version);
                }
                tokio::time::sleep(self.poll_interval).await;
            }
        };
        tokio::time::timeout(self.version_wait, polling)
            .await
            .unwrap_or_else(|_| {
                Err(StackitKmsError::NoUsableVersion {
                    key_id: key_id.to_string(),
                    waited: self.version_wait,
                })
            })
    }

    /// The keys in the key ring that have `label` and aren't deleted.
    pub(crate) async fn labelled(&self, label: &str) -> Result<impl Iterator<Item = Key> + use<'_>, KeyStoreError> {
        let keys = self.api.keys().await.map_err(KeyStoreError::backend)?;
        let label = label.to_string();
        Ok(keys
            .into_iter()
            .filter(move |key| key.display_name == label && key.state != KeyState::Deleted))
    }
}

/// The newest of `versions` that can sign.
pub(crate) fn newest_usable(versions: Vec<Version>) -> Option<Version> {
    versions
        .into_iter()
        .filter(Version::is_usable)
        .max_by_key(|version| version.number)
}

#[async_trait]
impl KeyProviderAdapter for StackitKmsKeyProvider {
    /// Signs in and reads the key ring.
    async fn test_connection(&self) -> Result<(), ConnectionError> {
        match self.api.key_ring_exists().await {
            Ok(true) => Ok(()),
            Ok(false) => Err(ConnectionError(
                "The key ring does not exist in the STACKIT project, or the service account may not see it".to_string(),
            )),
            Err(error) => Err(ConnectionError(error.to_string())),
        }
    }

    /// The key ring's ES256 signing keys. A deleted key is pending deletion, as STACKIT keeps it for 30 days.
    async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
        let keys = self
            .api
            .keys()
            .await
            .map_err(|error| ConnectionError(error.to_string()))?;
        Ok(ProviderKeys {
            keys: keys
                .into_iter()
                .filter(Key::signs_es256)
                .map(|key| ProviderKey {
                    id: key.id,
                    resource_name: None,
                    state: match key.state {
                        KeyState::Active => ProviderKeyState::Enabled,
                        KeyState::Deleted => ProviderKeyState::PendingDeletion,
                        KeyState::Other => ProviderKeyState::Unavailable,
                    },
                })
                .collect(),
            hidden_keys: 0,
        })
    }

    /// Refuses other algorithms than ES256, and labels STACKIT can't take as a display name.
    fn check_spec(&self, spec: &KeySpec) -> Result<(), KeyRefusal> {
        if spec.alg != JwsAlgorithm::ES256 {
            return Err(KeyRefusal::AlgorithmRefused(format!(
                "STACKIT KMS keys sign with ES256 only, not {}",
                spec.alg
            )));
        }
        match spec.label.as_deref().and_then(label_refusal) {
            Some(reason) => Err(KeyRefusal::InvalidLabel(reason)),
            None => Ok(()),
        }
    }

    /// Returns `<key id>/<version>` of the key's first version that can sign, waiting for it up to 30 seconds. The key
    /// is created, and waited for, also when the returned future is dropped.
    async fn create_key(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
        spec.validate()?;
        if spec.alg != JwsAlgorithm::ES256 {
            return Err(KeyStoreError::UnsupportedAlgorithm(spec.alg));
        }
        if let Some(reason) = spec.label.as_deref().and_then(label_refusal) {
            return Err(KeyStoreError::backend(KeyRefusal::InvalidLabel(reason)));
        }

        let provider = self.clone();
        tokio::spawn(async move { provider.create(spec.label).await })
            .await
            .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
    }

    /// The newest usable version of the key the label names, as `<key id>/<version>`.
    async fn find_key(&self, label: &str) -> Result<Option<KeyId>, KeyStoreError> {
        match self.signing_version(&KeyId::new(label)).await {
            Ok((key_id, version)) => Ok(Some(KeyId::new(format!("{key_id}/{}", version.number)))),
            Err(KeyStoreError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Two versions of one key name one key.
    fn same_key(&self, a: &KeyId, b: &KeyId) -> bool {
        KeyName::parse(a).key() == KeyName::parse(b).key()
    }
}

/// Builds STACKIT KMS providers from their settings, signed in to with the host's workload identity token.
pub struct StackitKmsFactory {
    token_file: PathBuf,
    kms_url: Url,
}

impl StackitKmsFactory {
    /// The factory for providers that sign in with the workload identity token in `token_file`.
    #[must_use]
    #[allow(clippy::missing_panics_doc, reason = "the KMS URL is a constant")]
    pub fn new(token_file: PathBuf) -> Self {
        Self {
            token_file,
            kms_url: Url::parse(KMS_URL).expect("a valid URL"),
        }
    }
}

impl KeyProviderFactory for StackitKmsFactory {
    fn build(&self, settings: &KeyProviderSettings) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError> {
        let KeyProviderSettings::StackitKms(settings) = settings;
        let sign_in = SignIn::workload_identity(self.token_file.clone(), settings.service_account_email.clone());
        let provider = StackitKmsKeyProvider::new(&self.kms_url, settings, sign_in)
            .map_err(|error| ConnectionError(format!("The STACKIT KMS client can't be set up: {error}")))?;
        Ok(Arc::new(provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_stackit::{FakeStackit, ES256};
    use identity_kit::jose::KeyType;

    fn es256(label: &str) -> KeySpec {
        KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label(label)
    }

    #[tokio::test]
    async fn creates_a_labelled_key_under_an_id_that_pins_its_version() {
        let stackit = FakeStackit::start().await;

        let key = stackit.provider().create_key(es256("issuer")).await.unwrap();

        assert_eq!(key, KeyId::new("00000000-0000-4000-8000-000000000001/1"));
    }

    #[tokio::test]
    async fn waits_until_a_new_key_has_a_version_it_can_sign_with() {
        let stackit = FakeStackit::start().await;
        stackit.state.lock().unwrap().polls_until_usable = Some(3);

        let result = stackit.provider().create_key(es256("issuer")).await;

        assert_eq!(result.unwrap(), KeyId::new("00000000-0000-4000-8000-000000000001/1"));
    }

    #[tokio::test]
    async fn deletes_a_new_key_that_does_not_get_a_version_it_can_sign_with_in_time() {
        let stackit = FakeStackit::start().await;
        stackit.state.lock().unwrap().polls_until_usable = None;

        let _ = stackit.provider().create_key(es256("issuer")).await;

        assert_eq!(stackit.key_state("00000000-0000-4000-8000-000000000001"), "deleted");
    }

    #[tokio::test]
    async fn names_a_new_key_it_could_neither_use_nor_delete() {
        let stackit = FakeStackit::start().await;
        {
            let mut fake = stackit.state.lock().unwrap();
            fake.polls_until_usable = None;
            fake.deletion_fails = true;
        }

        let error = stackit.provider().create_key(es256("issuer")).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "key store backend error: Key `00000000-0000-4000-8000-000000000001` got no version that can sign within 0.3 seconds; key \
             `00000000-0000-4000-8000-000000000001` could not be deleted again either (STACKIT KMS refused the \
             request (500): deletion failed), so delete it in STACKIT"
        );
    }

    #[tokio::test]
    async fn gives_a_label_that_only_a_deleted_key_has() {
        let stackit = FakeStackit::start().await;
        stackit.add_key("issuer", ES256, "deleted", &[("active", false)]);

        let result = stackit.provider().create_key(es256("issuer")).await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn gives_a_label_to_only_one_of_two_keys_created_with_it_at_once() {
        let stackit = FakeStackit::start().await;
        let provider = stackit.provider();

        let (first, second) = tokio::join!(
            provider.create_key(es256("issuer")),
            provider.create_key(es256("issuer"))
        );

        assert_eq!(
            [first.is_ok(), second.is_ok()]
                .iter()
                .filter(|created| **created)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn adopts_a_labelled_key_by_its_newest_usable_version() {
        let stackit = FakeStackit::start().await;
        let id = stackit.add_key(
            "issuer",
            ES256,
            "active",
            &[("active", false), ("active", false), ("active", true)],
        );

        let key = stackit.provider().find_key("issuer").await.unwrap();

        assert_eq!(key, Some(KeyId::new(format!("{id}/2"))));
    }

    #[tokio::test]
    async fn names_by_a_label_the_key_that_is_not_deleted() {
        let stackit = FakeStackit::start().await;
        stackit.add_key("issuer", ES256, "deleted", &[("active", false)]);
        let id = stackit.add_active_key("issuer");

        let key = stackit.provider().find_key("issuer").await.unwrap();

        assert_eq!(key, Some(KeyId::new(format!("{id}/1"))));
    }

    #[tokio::test]
    async fn refuses_a_label_that_several_keys_have() {
        let stackit = FakeStackit::start().await;
        stackit.add_active_key("issuer");
        stackit.add_active_key("issuer");

        let result = stackit.provider().find_key("issuer").await;

        assert!(matches!(result, Err(KeyStoreError::InvalidKey { .. })), "{result:?}");
    }

    #[test]
    fn takes_two_versions_of_one_key_for_one_key() {
        let provider = StackitKmsKeyProvider::new(
            &Url::parse(KMS_URL).unwrap(),
            &StackitKmsSettings {
                project_id: "project".to_string(),
                region: agent_kms::key_provider::settings::StackitRegion::Eu01,
                key_ring_id: "key-ring".to_string(),
                service_account_email: "unicore-kms@sa.stackit.cloud".to_string(),
            },
            SignIn::workload_identity(PathBuf::from("token"), "unicore-kms@sa.stackit.cloud".to_string()),
        )
        .unwrap();

        assert!(provider.same_key(
            &KeyId::new("0b6a7f8e-3f4c-4d2a-9b1e-5c8d7e6f5a4b/1"),
            &KeyId::new("0b6a7f8e-3f4c-4d2a-9b1e-5c8d7e6f5a4b/2")
        ));
    }

    #[tokio::test]
    async fn refuses_keys_for_another_algorithm_than_es256() {
        let stackit = FakeStackit::start().await;

        let refusal = stackit
            .provider()
            .check_spec(&KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA));

        assert_eq!(
            refusal,
            Err(KeyRefusal::AlgorithmRefused(
                "STACKIT KMS keys sign with ES256 only, not EdDSA".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn is_reachable_when_the_key_ring_exists() {
        let stackit = FakeStackit::start().await;

        let result = stackit.provider().test_connection().await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn is_unreachable_when_the_key_ring_does_not_exist() {
        let stackit = FakeStackit::start().await;

        let error = stackit
            .provider_for("1f0e7b52-5d2a-4f0e-8a51-3b6d1e2f4a7c")
            .test_connection()
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "The key ring does not exist in the STACKIT project, or the service account may not see it"
        );
    }

    #[tokio::test]
    async fn lists_the_keys_that_sign_with_es256_and_whether_they_can_be_used() {
        let stackit = FakeStackit::start().await;
        let active = stackit.add_active_key("active");
        let deleted = stackit.add_key("deleted", ES256, "deleted", &[("active", false)]);
        let creating = stackit.add_key("creating", ES256, "creating", &[]);
        stackit.add_key("p-384", "ecdsa_p384_sha384", "active", &[("active", false)]);

        let listing = stackit.provider().list_keys().await.unwrap();

        let states: Vec<_> = listing.keys.into_iter().map(|key| (key.id, key.state)).collect();
        assert_eq!(
            states,
            [
                (active, ProviderKeyState::Enabled),
                (deleted, ProviderKeyState::PendingDeletion),
                (creating, ProviderKeyState::Unavailable),
            ]
        );
    }

    #[tokio::test]
    async fn signs_in_again_when_stackit_refuses_the_access_token() {
        let stackit = FakeStackit::start().await;
        stackit.state.lock().unwrap().refused_tokens = 1;

        stackit.provider().test_connection().await.unwrap();

        assert_eq!(stackit.sign_ins(), 2);
    }

    #[tokio::test]
    async fn gives_up_when_stackit_refuses_the_new_access_token_too() {
        let stackit = FakeStackit::start().await;
        stackit.state.lock().unwrap().refused_tokens = 2;

        let error = stackit.provider().test_connection().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "STACKIT KMS refused the request (401): the access token expired"
        );
    }
}
