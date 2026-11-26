//! Creating, adopting, deleting and listing keys, and handing them out to sign with. A key is authorized on its own id
//! before its provider is asked anything.

use crate::adapter::KeyRefusal;
use crate::key::{
    aggregate::KeyReference,
    command::{KeyCommand, KeyOperation},
    error::KeyError,
    views::AllKeysView,
};
use crate::key_provider::aggregate::KeyProviderType;
use crate::state::{added_provider, all_keys, AddedProvider, KeyProviderAccessError, KmsState};
use agent_shared::handlers::{
    authorize_command, command_handler, public_query_handler, query_handler, CommandHandlerError, QueryHandlerError,
};
use cqrs_es::persist::PersistenceError;
use identity_kit::{
    jose::{JwsAlgorithm, KeyType},
    keys::{KeyId, KeySigner, KeySpec, KeyStore, KeyStoreError},
};
use shared_kernel::authorization::{AuthorizationError, Caller};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// The algorithms keys are created for, with the type and JWK curve of their keys.
const ALGORITHMS: [(JwsAlgorithm, KeyType, &str); 2] = [
    (JwsAlgorithm::ES256, KeyType::P256, "P-256"),
    (JwsAlgorithm::EdDSA, KeyType::Ed25519, "Ed25519"),
];

/// A key this instance recorded: its own id for the key, and what names the key in its provider.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedKey {
    pub id: String,
    pub reference: KeyReference,
}

/// Why a key could not be created or adopted.
#[derive(Debug, Error)]
pub enum RecordKeyError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Access(#[from] KeyProviderAccessError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("Keys can't be created for {0}")]
    UnsupportedAlgorithm(JwsAlgorithm),
    #[error("The key labelled `{0}` is neither a P-256 nor an Ed25519 key")]
    UnsupportedKey(String),
    #[error(transparent)]
    Refused(#[from] KeyRefusal),
    #[error(transparent)]
    Provider(#[from] KeyStoreError),
    #[error("No key in the provider is labelled `{0}`")]
    LabelNotFound(String),
    #[error("The key labelled `{0}` is already recorded")]
    AlreadyRecorded(String),
    #[error(transparent)]
    Record(#[from] CommandHandlerError<KeyError>),
    /// The provider created the key, but it was neither recorded nor deleted again, so the provider still has it.
    #[error("The provider created key `{key}`, but it could not be recorded ({cause}) nor deleted again ({deletion})")]
    Abandoned {
        key: KeyId,
        cause: Box<RecordKeyError>,
        deletion: KeyStoreError,
    },
}

/// Creates a key in an added provider and records it. The private key never leaves the provider.
///
/// Once the provider is asked to create the key, creating and recording it finish even when the returned future is
/// dropped. A key that can't be recorded is deleted from the provider again, which frees its label too.
///
/// # Errors
///
/// Fails when the caller isn't authorized, the provider isn't added, keys aren't created for `algorithm`, the
/// provider refuses or fails the key, or the key can't be recorded.
pub async fn create_key(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
    algorithm: JwsAlgorithm,
    label: Option<String>,
) -> Result<RecordedKey, RecordKeyError> {
    let id = Uuid::new_v4().to_string();
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &id,
        &KeyOperation::CreateKey,
    )
    .await
    .map_err(RecordKeyError::Authorization)?;

    let AddedProvider { settings, adapter } = added_provider(state, caller.clone(), provider_type).await?;
    let Some((_, key_type, _)) = ALGORITHMS.into_iter().find(|(candidate, ..)| *candidate == algorithm) else {
        return Err(RecordKeyError::UnsupportedAlgorithm(algorithm));
    };
    let spec = match label.clone() {
        Some(label) => KeySpec::new(key_type, algorithm).with_label(label),
        None => KeySpec::new(key_type, algorithm),
    };
    adapter.check_spec(&spec)?;

    let authorization_checker = state.authorization_checker.clone();
    let commands = state.command.keys.clone();
    let creation = tokio::spawn(async move {
        let key = adapter.create_key(spec).await?;

        let recorded = async {
            let reference = KeyReference {
                provider_type,
                settings,
                provider_key_id: key.clone(),
                label,
                algorithm,
                public_key: adapter.public_jwk(&key).await?,
            };
            command_handler(
                authorization_checker,
                caller,
                &id,
                &commands,
                KeyCommand::CreateKey {
                    reference: reference.clone(),
                },
            )
            .await?;
            Ok(RecordedKey { id, reference })
        }
        .await;

        match recorded {
            Ok(recorded) => Ok(recorded),
            Err(cause) => match adapter.delete(&key).await {
                Ok(()) => Err(cause),
                Err(deletion) => Err(RecordKeyError::Abandoned {
                    key,
                    cause: Box::new(cause),
                    deletion,
                }),
            },
        }
    });

    creation
        .await
        .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

/// Records a key that exists in an added provider under `label` but that this instance didn't record, such as a key
/// whose creation was interrupted after the provider created it. A key recorded under another id that the provider
/// says names the same key, such as another version of it, counts as recorded.
///
/// # Errors
///
/// Fails when the caller isn't authorized, the provider isn't added, no key has the label, the key is recorded
/// already, it is not a key keys are created for, or the provider refuses it.
pub async fn adopt_key(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
    label: String,
) -> Result<RecordedKey, RecordKeyError> {
    let id = Uuid::new_v4().to_string();
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &id,
        &KeyOperation::AdoptKey,
    )
    .await
    .map_err(RecordKeyError::Authorization)?;

    let AddedProvider { settings, adapter } = added_provider(state, caller.clone(), provider_type).await?;
    let Some(key) = adapter.find_key(&label).await? else {
        return Err(RecordKeyError::LabelNotFound(label));
    };
    if all_keys(state).await?.keys.values().any(|reference| {
        reference.provider_type == provider_type
            && reference.settings == settings
            && adapter.same_key(&reference.provider_key_id, &key)
    }) {
        return Err(RecordKeyError::AlreadyRecorded(label));
    }

    let public_key = adapter.public_jwk(&key).await?;
    let Some((algorithm, key_type, _)) = ALGORITHMS
        .into_iter()
        .find(|(.., curve)| public_key.crv() == Some(curve))
    else {
        return Err(RecordKeyError::UnsupportedKey(label));
    };
    adapter.check_spec(&KeySpec::new(key_type, algorithm).with_label(label.clone()))?;

    let reference = KeyReference {
        provider_type,
        settings,
        provider_key_id: key,
        label: Some(label),
        algorithm,
        public_key,
    };
    command_handler(
        state.authorization_checker.clone(),
        caller,
        &id,
        &state.command.keys,
        KeyCommand::AdoptKey {
            reference: reference.clone(),
        },
    )
    .await?;
    Ok(RecordedKey { id, reference })
}

/// Why a key could not be deleted.
#[derive(Debug, Error)]
pub enum DeleteKeyError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("There is no key `{0}`")]
    NotFound(String),
    /// Something still signs with the key, as [`KeyUsage`](crate::key::usage::KeyUsage) tells.
    #[error("Key `{id}` is the key of `{user}`; remove it from there first")]
    InUse { id: String, user: String },
    #[error("This instance has no adapter for `{0}` key providers")]
    Unsupported(KeyProviderType),
    #[error(transparent)]
    Provider(KeyStoreError),
    #[error(transparent)]
    Record(#[from] CommandHandlerError<KeyError>),
}

/// Deletes a key from its provider, which frees its label, and records that it is deleted. The provider doesn't need
/// to be added anymore: the key is reached with the settings it was recorded with. A key the provider no longer has counts as deleted there, so an interrupted deletion can be
/// retried. A key that something still signs with is kept.
///
/// # Errors
///
/// Fails when the caller isn't authorized, there is no such key, something signs with it, this instance has no
/// adapter for its provider, or the provider fails to delete it.
pub async fn delete_key(state: &KmsState, caller: Caller, id: &str) -> Result<(), DeleteKeyError> {
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        id,
        &KeyOperation::DeleteKey,
    )
    .await
    .map_err(DeleteKeyError::Authorization)?;

    let Some(reference) = public_query_handler(id, &state.query.key)
        .await?
        .and_then(|view| view.reference)
    else {
        return Err(DeleteKeyError::NotFound(id.to_string()));
    };
    if let Some(user) = state.key_usage.user(id).await? {
        return Err(DeleteKeyError::InUse {
            id: id.to_string(),
            user,
        });
    }
    let Some(adapter) = state.adapters.get(reference.provider_type, reference.settings.as_ref()) else {
        return Err(DeleteKeyError::Unsupported(reference.provider_type));
    };
    match adapter.delete(&reference.provider_key_id).await {
        Ok(()) | Err(KeyStoreError::NotFound(_)) => {}
        Err(error) => return Err(DeleteKeyError::Provider(error)),
    }

    command_handler(
        state.authorization_checker.clone(),
        caller,
        id,
        &state.command.keys,
        KeyCommand::DeleteKey,
    )
    .await?;
    Ok(())
}

/// Why a key can't be given to sign with.
#[derive(Debug, Error)]
pub enum SigningKeyError {
    /// The key's record couldn't be read. The key may still sign.
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// The key was never recorded, was deleted, or its provider no longer holds it.
    #[error("There is no key `{0}`")]
    NotFound(String),
    /// This instance has no adapter for the key's provider.
    #[error("This instance has no adapter for `{0}` key providers")]
    Unsupported(KeyProviderType),
    /// The provider couldn't be asked whether it holds the key. The key may still sign.
    #[error(transparent)]
    Provider(KeyStoreError),
    /// The provider can't sign with the key as it was recorded, such as with another algorithm.
    #[error(transparent)]
    Unusable(KeyStoreError),
}

/// The key `id` as a signer, signing inside its provider with the algorithm the key was recorded for. Its public key
/// is the recorded one, so the provider is only asked whether it still holds the key. The provider is reached with the
/// settings the key was recorded with, so the key signs whether or not its provider is still added.
///
/// Not authorized: it is asked for by whatever signs for something that names the key, which authorized that.
///
/// # Errors
///
/// Fails when there is no such key, this instance has no adapter for its provider, the provider no longer holds it or
/// can't be asked, or the provider can't sign with it.
pub async fn signing_key(state: &KmsState, id: &str) -> Result<KeySigner<Arc<dyn KeyStore>>, SigningKeyError> {
    let Some(reference) = public_query_handler(id, &state.query.key)
        .await?
        .and_then(|view| view.reference)
    else {
        return Err(SigningKeyError::NotFound(id.to_string()));
    };
    let Some(adapter) = state.adapters.get(reference.provider_type, reference.settings.as_ref()) else {
        return Err(SigningKeyError::Unsupported(reference.provider_type));
    };
    let store: Arc<dyn KeyStore> = adapter.clone();
    match store.exists(&reference.provider_key_id).await {
        Ok(true) => {}
        Ok(false) => return Err(SigningKeyError::NotFound(id.to_string())),
        Err(error) => return Err(SigningKeyError::Provider(error)),
    }

    KeySigner::from_public_jwk(
        store,
        reference.provider_key_id,
        reference.algorithm,
        reference.public_key,
    )
    .map_err(SigningKeyError::Unusable)
}

/// Returns the keys this instance recorded and didn't delete, in the order they were recorded.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the keys can't be loaded.
pub async fn get_keys(state: &KmsState, caller: Caller) -> Result<Vec<RecordedKey>, QueryHandlerError> {
    let view = query_handler(
        state.authorization_checker.clone(),
        caller,
        AllKeysView::ID,
        None,
        &state.query.all_keys,
    )
    .await?
    .unwrap_or_default();

    Ok(view
        .keys
        .into_iter()
        .map(|(id, reference)| RecordedKey { id, reference })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{
        test_utils::InMemoryFactory, ConnectionError, InMemoryKeyStore, KeyProviderAdapter, KeyProviderAdapters,
        ProviderKeys, UnreachableKeyProvider,
    };
    use crate::key::aggregate::{tests::reference, Key};
    use crate::key::usage::{KeyUsage, Unused};
    use crate::key_provider::settings::tests::stackit;
    use crate::key_provider::{aggregate::KeyProviders, command::KeyProviderCommand};
    use crate::kms_state;
    use crate::state::tests::{add_stackit, stackit_state};
    use agent_shared::application_state::Command;
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use cqrs_es::AggregateError;
    use identity_kit::jose::{encode_compact, CompactJws, DefaultVerifier, Jwk, JwsHeader};
    use identity_kit::keys::KeyStoreCapabilities;
    use shared_kernel::authorization::{AuthorizationChecker, AuthorizationRequest};
    use shared_kernel::event_bus::EventBusHandle;
    use std::{collections::HashMap, sync::Arc, time::Duration};

    /// The in-memory provider, refusing every key with `refusal`, taking `creation_delay` to create one, and failing
    /// every deletion when `deletion_fails`. With `versioned`, ids that agree before a `/` name one key, as versions
    /// of it.
    #[derive(Default)]
    struct TestProvider {
        store: InMemoryKeyStore,
        refusal: Option<KeyRefusal>,
        creation_delay: Duration,
        deletion_fails: bool,
        versioned: bool,
    }

    #[async_trait]
    impl KeyStore for TestProvider {
        fn capabilities(&self) -> KeyStoreCapabilities {
            self.store.capabilities()
        }

        async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
            self.store.public_jwk(key).await
        }

        async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
            self.store.sign(key, alg, data).await
        }

        async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
            self.store.exists(key).await
        }

        async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
            self.store.generate(spec).await
        }

        async fn delete(&self, key: &KeyId) -> Result<(), KeyStoreError> {
            if self.deletion_fails {
                return Err(KeyStoreError::backend(std::io::Error::other("throttled")));
            }
            self.store.delete(key).await
        }
    }

    #[async_trait]
    impl KeyProviderAdapter for TestProvider {
        async fn test_connection(&self) -> Result<(), ConnectionError> {
            Ok(())
        }

        async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
            Ok(ProviderKeys::default())
        }

        fn check_spec(&self, _spec: &KeySpec) -> Result<(), KeyRefusal> {
            self.refusal.clone().map_or(Ok(()), Err)
        }

        async fn create_key(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
            tokio::time::sleep(self.creation_delay).await;
            self.store.create_key(spec).await
        }

        async fn find_key(&self, label: &str) -> Result<Option<KeyId>, KeyStoreError> {
            self.store.find_key(label).await
        }

        fn same_key(&self, a: &KeyId, b: &KeyId) -> bool {
            let key = |id: &KeyId| id.as_str().split('/').next().unwrap_or_default().to_string();
            if self.versioned {
                key(a) == key(b)
            } else {
                a == b
            }
        }
    }

    struct DenyAll;

    #[async_trait]
    impl AuthorizationChecker for DenyAll {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            Err(AuthorizationError::Forbidden)
        }
    }

    /// Records nothing: every key command fails.
    struct FailingKeyCommands;

    #[async_trait]
    impl Command<Key> for FailingKeyCommands {
        async fn execute_with_metadata(
            &self,
            _aggregate_id: &str,
            _command: KeyCommand,
            _metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<KeyError>> {
            Err(AggregateError::UnexpectedError("the event store is down".into()))
        }
    }

    /// Something signs with every key.
    struct SignsWithEveryKey;

    #[async_trait]
    impl KeyUsage for SignsWithEveryKey {
        async fn user(&self, _id: &str) -> Result<Option<String>, PersistenceError> {
            Ok(Some("did:web:example.org#key".to_string()))
        }
    }

    /// A state with `provider` added as the in-memory provider.
    async fn state(provider: &Arc<TestProvider>) -> KmsState {
        let adapters = [(
            KeyProviderType::InMemory,
            provider.clone() as Arc<dyn KeyProviderAdapter>,
        )]
        .into();
        let state = kms_state(&InMemory, &EventBusHandle::default(), true, adapters, Arc::new(Unused)).await;
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::InMemory,
                name: "Local keys".to_string(),
                settings: None,
            },
        )
        .await
        .unwrap();
        state
    }

    async fn create(state: &KmsState, label: Option<&str>) -> Result<RecordedKey, RecordKeyError> {
        create_key(
            state,
            Caller::Internal,
            KeyProviderType::InMemory,
            JwsAlgorithm::ES256,
            label.map(str::to_string),
        )
        .await
    }

    async fn adopt(state: &KmsState, label: &str) -> Result<RecordedKey, RecordKeyError> {
        adopt_key(state, Caller::Internal, KeyProviderType::InMemory, label.to_string()).await
    }

    /// Records `reference` as the key `id`, without asking its provider.
    async fn record(state: &KmsState, id: &str, reference: KeyReference) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            id,
            &state.command.keys,
            KeyCommand::CreateKey { reference },
        )
        .await
        .unwrap();
    }

    async fn recorded_ids(state: &KmsState) -> Vec<String> {
        get_keys(state, Caller::Internal)
            .await
            .unwrap()
            .into_iter()
            .map(|key| key.id)
            .collect()
    }

    #[tokio::test]
    async fn creates_no_key_that_it_may_not_record() {
        let provider = Arc::new(TestProvider::default());
        let mut state = state(&provider).await;
        state.authorization_checker = Arc::new(DenyAll);

        let _ = create(&state, None).await;

        assert!(provider.store.key_ids().is_empty());
    }

    #[tokio::test]
    async fn records_the_public_key_the_provider_holds() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;

        let key = create(&state, Some("issuer")).await.unwrap();

        assert_eq!(
            key.reference.public_key,
            provider.store.public_jwk(&KeyId::new("issuer")).await.unwrap()
        );
    }

    #[tokio::test]
    async fn records_nothing_when_the_provider_fails() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("issuer"))
            .await
            .unwrap();

        let _ = create(&state, Some("issuer")).await;

        assert!(recorded_ids(&state).await.is_empty());
    }

    #[tokio::test]
    async fn records_the_key_although_the_caller_gives_up() {
        let provider = Arc::new(TestProvider {
            creation_delay: Duration::from_millis(100),
            ..TestProvider::default()
        });
        let state = state(&provider).await;
        let _ = tokio::time::timeout(Duration::from_millis(10), create(&state, None)).await;

        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(recorded_ids(&state).await.len(), 1);
    }

    #[tokio::test]
    async fn deletes_a_key_it_could_not_record() {
        let provider = Arc::new(TestProvider::default());
        let mut state = state(&provider).await;
        state.command.keys = Arc::new(FailingKeyCommands);

        let _ = create(&state, Some("issuer")).await;

        assert!(provider.store.key_ids().is_empty());
    }

    #[tokio::test]
    async fn names_a_key_it_could_neither_record_nor_delete() {
        let provider = Arc::new(TestProvider {
            deletion_fails: true,
            ..TestProvider::default()
        });
        let mut state = state(&provider).await;
        state.command.keys = Arc::new(FailingKeyCommands);

        let result = create(&state, Some("issuer")).await;

        assert!(matches!(result, Err(RecordKeyError::Abandoned { key, .. }) if key == KeyId::new("issuer")));
    }

    #[tokio::test]
    async fn keeps_a_key_its_provider_fails_to_delete() {
        let provider = Arc::new(TestProvider {
            deletion_fails: true,
            ..TestProvider::default()
        });
        let state = state(&provider).await;
        let key = create(&state, Some("issuer")).await.unwrap();

        let _ = delete_key(&state, Caller::Internal, &key.id).await;

        assert_eq!(recorded_ids(&state).await, [key.id]);
    }

    #[tokio::test]
    async fn refuses_a_label_the_provider_cannot_give() {
        let provider = Arc::new(TestProvider {
            refusal: Some(KeyRefusal::InvalidLabel("no spaces".to_string())),
            ..TestProvider::default()
        });
        let state = state(&provider).await;

        let result = create(&state, Some("my issuer")).await;

        assert!(matches!(
            result,
            Err(RecordKeyError::Refused(KeyRefusal::InvalidLabel(_)))
        ));
    }

    #[tokio::test]
    async fn refuses_an_algorithm_it_does_not_create_keys_for() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;

        let result = create_key(
            &state,
            Caller::Internal,
            KeyProviderType::InMemory,
            JwsAlgorithm::ES256K,
            None,
        )
        .await;

        assert!(matches!(
            result,
            Err(RecordKeyError::UnsupportedAlgorithm(JwsAlgorithm::ES256K))
        ));
    }

    #[tokio::test]
    async fn refuses_an_algorithm_the_provider_refuses() {
        let provider = Arc::new(TestProvider {
            refusal: Some(KeyRefusal::AlgorithmRefused("4096 bytes at most".to_string())),
            ..TestProvider::default()
        });
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA).with_label("issuer"))
            .await
            .unwrap();

        let result = adopt(&state, "issuer").await;

        assert!(matches!(
            result,
            Err(RecordKeyError::Refused(KeyRefusal::AlgorithmRefused(_)))
        ));
    }

    #[tokio::test]
    async fn adopts_a_key_its_provider_has_under_the_label() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::Ed25519, JwsAlgorithm::EdDSA).with_label("issuer"))
            .await
            .unwrap();

        let key = adopt(&state, "issuer").await.unwrap();

        assert_eq!(
            (key.reference.provider_key_id, key.reference.algorithm),
            (KeyId::new("issuer"), JwsAlgorithm::EdDSA)
        );
    }

    #[tokio::test]
    async fn refuses_to_adopt_a_recorded_key() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        create(&state, Some("issuer")).await.unwrap();

        let result = adopt(&state, "issuer").await;

        assert!(matches!(result, Err(RecordKeyError::AlreadyRecorded(_))));
    }

    #[tokio::test]
    async fn deletes_a_key_from_its_provider() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        let key = create(&state, Some("issuer")).await.unwrap();

        delete_key(&state, Caller::Internal, &key.id).await.unwrap();

        assert!(provider.store.key_ids().is_empty());
    }

    #[tokio::test]
    async fn records_a_key_its_provider_no_longer_has_as_deleted() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        let key = create(&state, Some("issuer")).await.unwrap();
        provider.store.delete(&KeyId::new("issuer")).await.unwrap();

        delete_key(&state, Caller::Internal, &key.id).await.unwrap();

        assert!(recorded_ids(&state).await.is_empty());
    }

    #[tokio::test]
    async fn refuses_to_delete_the_key_of_a_verification_method() {
        let provider = Arc::new(TestProvider::default());
        let mut state = state(&provider).await;
        state.key_usage = Arc::new(SignsWithEveryKey);
        let key = create(&state, Some("issuer")).await.unwrap();

        let result = delete_key(&state, Caller::Internal, &key.id).await;

        assert!(matches!(result, Err(DeleteKeyError::InUse { user, .. }) if user == "did:web:example.org#key"));
    }

    #[tokio::test]
    async fn gives_a_key_to_sign_with_with_the_adapter_of_its_provider() {
        let provider = Arc::new(TestProvider::default());
        let mut state = state(&provider).await;
        let aws = Arc::new(InMemoryKeyStore::new());
        let adapters: KeyProviderAdapters = [
            (
                KeyProviderType::InMemory,
                provider.clone() as Arc<dyn KeyProviderAdapter>,
            ),
            (KeyProviderType::AwsKmsAmbient, aws.clone()),
        ]
        .into();
        state.adapters = Arc::new(adapters);
        let key = aws
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256))
            .await
            .unwrap();
        let public_key = aws.public_jwk(&key).await.unwrap();
        record(
            &state,
            "key-id",
            KeyReference {
                provider_type: KeyProviderType::AwsKmsAmbient,
                provider_key_id: key,
                public_key: public_key.clone(),
                ..reference()
            },
        )
        .await;

        let signer = signing_key(&state, "key-id").await.unwrap();

        let token = encode_compact(&JwsHeader::new(JwsAlgorithm::ES256), b"{}", &signer)
            .await
            .unwrap();
        assert!(CompactJws::decode(&token)
            .unwrap()
            .verify(&DefaultVerifier, &public_key)
            .is_ok());
    }

    #[tokio::test]
    async fn an_unknown_key_cannot_sign() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;

        let result = signing_key(&state, "missing").await;

        assert!(matches!(result, Err(SigningKeyError::NotFound(id)) if id == "missing"));
    }

    #[tokio::test]
    async fn a_key_without_an_adapter_for_its_provider_cannot_sign() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        record(
            &state,
            "key-id",
            KeyReference {
                provider_type: KeyProviderType::AwsKmsAmbient,
                ..reference()
            },
        )
        .await;

        let result = signing_key(&state, "key-id").await;

        assert!(matches!(
            result,
            Err(SigningKeyError::Unsupported(KeyProviderType::AwsKmsAmbient))
        ));
    }

    #[tokio::test]
    async fn a_key_its_provider_no_longer_holds_cannot_sign() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        let key = create(&state, Some("issuer")).await.unwrap();
        provider.store.delete(&KeyId::new("issuer")).await.unwrap();

        let result = signing_key(&state, &key.id).await;

        assert!(matches!(result, Err(SigningKeyError::NotFound(_))));
    }

    #[tokio::test]
    async fn a_key_whose_provider_cannot_be_reached_is_not_reported_missing() {
        let provider = Arc::new(TestProvider::default());
        let mut state = state(&provider).await;
        let adapters: KeyProviderAdapters = [(
            KeyProviderType::AwsKmsAmbient,
            Arc::new(UnreachableKeyProvider("connection refused".to_string())) as Arc<dyn KeyProviderAdapter>,
        )]
        .into();
        state.adapters = Arc::new(adapters);
        record(
            &state,
            "key-id",
            KeyReference {
                provider_type: KeyProviderType::AwsKmsAmbient,
                ..reference()
            },
        )
        .await;

        let result = signing_key(&state, "key-id").await;

        assert!(matches!(result, Err(SigningKeyError::Provider(_))));
    }

    #[tokio::test]
    async fn a_key_recorded_for_another_algorithm_than_its_public_key_cannot_sign() {
        let provider = Arc::new(TestProvider::default());
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("issuer"))
            .await
            .unwrap();
        record(
            &state,
            "key-id",
            KeyReference {
                algorithm: JwsAlgorithm::EdDSA,
                ..reference()
            },
        )
        .await;

        let result = signing_key(&state, "key-id").await;

        assert!(matches!(result, Err(SigningKeyError::Unusable(_))));
    }

    async fn create_in_stackit(state: &KmsState) -> RecordedKey {
        create_key(
            state,
            Caller::Internal,
            KeyProviderType::StackitKms,
            JwsAlgorithm::ES256,
            Some("issuer".to_string()),
        )
        .await
        .unwrap()
    }

    /// Removes the STACKIT KMS provider although it has keys, which `remove_key_provider` refuses.
    async fn remove_stackit(state: &KmsState) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            KeyProviderCommand::RemoveKeyProvider {
                provider_type: KeyProviderType::StackitKms,
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_key_signs_after_its_provider_is_removed() {
        let state = stackit_state(&InMemoryFactory::default(), stackit("key-ring")).await;
        let key = create_in_stackit(&state).await;
        remove_stackit(&state).await;

        let result = signing_key(&state, &key.id).await;

        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn a_key_signs_with_the_settings_it_was_created_with() {
        let state = stackit_state(&InMemoryFactory::default(), stackit("key-ring")).await;
        let key = create_in_stackit(&state).await;
        remove_stackit(&state).await;
        add_stackit(&state, stackit("other-key-ring")).await;

        let result = signing_key(&state, &key.id).await;

        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn refuses_to_adopt_another_version_of_a_recorded_key() {
        let provider = Arc::new(TestProvider {
            versioned: true,
            ..TestProvider::default()
        });
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("issuer"))
            .await
            .unwrap();
        record(
            &state,
            "key-id",
            KeyReference {
                provider_key_id: KeyId::new("issuer/1"),
                label: None,
                ..reference()
            },
        )
        .await;

        let result = adopt(&state, "issuer").await;

        assert!(matches!(result, Err(RecordKeyError::AlreadyRecorded(_))));
    }

    #[tokio::test]
    async fn refuses_to_adopt_by_a_label_the_provider_refuses() {
        let provider = Arc::new(TestProvider {
            refusal: Some(KeyRefusal::InvalidLabel("reads as a key id".to_string())),
            ..TestProvider::default()
        });
        let state = state(&provider).await;
        provider
            .store
            .generate(KeySpec::new(KeyType::P256, JwsAlgorithm::ES256).with_label("issuer"))
            .await
            .unwrap();

        let result = adopt(&state, "issuer").await;

        assert!(matches!(
            result,
            Err(RecordKeyError::Refused(KeyRefusal::InvalidLabel(_)))
        ));
    }
}
