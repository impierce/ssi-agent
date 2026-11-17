use crate::adapter::{ConnectionError, InvalidLabel, KeyProviderAdapter, KeyProviderFactories, ProviderKeys};
use crate::key::{
    aggregate::{Key, KeyReference},
    command::{CreateKey, KeyCommand},
    error::KeyError,
    views::{AllKeysView, KeyView},
};
use crate::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    command::{ListKeyProviderKeys, TestKeyProviderConnection},
    services::KeyProviderServices,
    settings::KeyProviderSettings,
    views::KeyProvidersView,
};
use agent_shared::{
    application_state::{CommandHandler, CqrsComponentBuilder},
    handlers::{
        authorize_command, command_handler, public_query_handler, query_handler, CommandHandlerError, QueryHandlerError,
    },
};
use cqrs_es::persist::PersistenceError;
use identity_kit::{
    jose::{JwsAlgorithm, KeyType},
    keys::{KeyId, KeySpec, KeyStoreError},
};
use shared_kernel::authorization::{
    AllowAllAuthorizationChecker, AuthorizationChecker, AuthorizationError, Caller, CommandOperation, QueryOperation,
};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tracing::error;
use uuid::Uuid;

impl QueryOperation for KeyProvidersView {
    const OPERATION_NAME: &'static str = "kms.key_providers.list";
}

impl QueryOperation for KeyView {
    const OPERATION_NAME: &'static str = "kms.keys.get";
}

impl QueryOperation for AllKeysView {
    const OPERATION_NAME: &'static str = "kms.keys.list";
}

#[derive(Clone)]
pub struct KmsState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
    pub factories: Arc<KeyProviderFactories>,
    /// The adapters built so far, by provider type, with the settings each was built from.
    built: Arc<Mutex<HashMap<KeyProviderType, BuiltAdapter>>>,
    /// Whether development-only providers, such as the in-memory provider, may be used.
    pub development_providers_allowed: bool,
}

struct BuiltAdapter {
    settings: Option<KeyProviderSettings>,
    adapter: Arc<dyn KeyProviderAdapter>,
}

#[derive(Clone)]
pub struct CommandHandlers {
    pub key_providers: CommandHandler<KeyProviders>,
    pub keys: CommandHandler<Key>,
}

#[derive(Clone)]
pub struct Queries {
    pub key_providers: Arc<dyn DynViewRepository<KeyProvidersView, KeyProviders>>,
    pub key: Arc<dyn DynViewRepository<KeyView, Key>>,
    pub all_keys: Arc<dyn DynViewRepository<AllKeysView, Key>>,
}

/// Constructs the CQRS components of the KMS bounded context.
///
/// `factories` holds a factory for every provider type this instance can connect to.
pub async fn kms_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_bus: &EventBusHandle,
    development_providers_allowed: bool,
    factories: KeyProviderFactories,
) -> KmsState {
    let services = Arc::new(KeyProviderServices::new(development_providers_allowed));

    // There is only one key providers aggregate, so its own view already lists every provider and the
    // all-aggregates view is not needed.
    let (key_providers_command_handler, key_providers, _) = builder
        .commands_and_queries::<KeyProvidersView, KeyProviders, KeyProvidersView>(services, vec![event_bus.query()])
        .await;
    let (keys_command_handler, key, all_keys) = builder
        .commands_and_queries::<KeyView, Key, AllKeysView>((), vec![event_bus.query()])
        .await;

    KmsState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: CommandHandlers {
            key_providers: key_providers_command_handler,
            keys: keys_command_handler,
        },
        query: Queries {
            key_providers,
            key,
            all_keys,
        },
        factories: Arc::new(factories),
        built: Arc::default(),
        development_providers_allowed,
    }
}

/// Returns every key provider this instance is connected to, by type, in the order they were added.
///
/// A development-only provider added while running with the `development` profile is left out when running with
/// another profile on the same event store.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the providers can't be loaded.
pub async fn get_key_providers(state: &KmsState, caller: Caller) -> Result<KeyProvidersView, QueryHandlerError> {
    let mut view = query_handler(
        state.authorization_checker.clone(),
        caller,
        KeyProviders::ID,
        None,
        &state.query.key_providers,
    )
    .await?
    .unwrap_or_default();
    view.providers
        .retain(|provider_type, _| KeyProviders::is_visible(*provider_type, state.development_providers_allowed));
    Ok(view)
}

#[derive(Debug, Error)]
pub enum TestConnectionError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Query(#[from] QueryHandlerError),
    #[error("No `{0}` key provider has been added")]
    NotFound(KeyProviderType),
    #[error("This instance has no adapter for `{0}` key providers")]
    Unsupported(KeyProviderType),
}

/// The adapter of an added provider, or why it can't be reached.
type ProviderAdapter = Result<Arc<dyn KeyProviderAdapter>, ConnectionError>;

/// Returns the name of the provider of the given type, or `None` if it hasn't been added or has been removed.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the providers can't be loaded.
pub async fn get_key_provider(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<Option<String>, QueryHandlerError> {
    Ok(get_key_providers(state, caller)
        .await?
        .providers
        .shift_remove(&provider_type))
}

/// Checks whether an added provider is reachable. The outcome is returned, not stored.
///
/// The outer `Result` fails when the check couldn't be run; the inner one carries the provider's answer.
///
/// # Errors
///
/// Fails when the caller isn't authorized, the provider hasn't been added, or this instance has no adapter for it.
pub async fn test_key_provider_connection(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<Result<(), ConnectionError>, TestConnectionError> {
    Ok(
        match added_provider_adapter(state, caller, provider_type, &TestKeyProviderConnection).await? {
            Ok(adapter) => adapter.test_connection().await,
            Err(error) => Err(error),
        },
    )
}

/// Lists the keys that exist in an added provider. Nothing is stored.
///
/// The outer `Result` fails when the request couldn't be made; the inner one carries the provider's answer.
///
/// # Errors
///
/// Fails when the caller isn't authorized, the provider hasn't been added, or this instance has no adapter for it.
pub async fn list_key_provider_keys(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<Result<ProviderKeys, ConnectionError>, TestConnectionError> {
    Ok(
        match added_provider_adapter(state, caller, provider_type, &ListKeyProviderKeys).await? {
            Ok(adapter) => adapter.list_keys().await,
            Err(error) => Err(error),
        },
    )
}

/// The adapter of an added provider, after checking that the caller may perform `operation` on it.
///
/// The adapter is built from the settings the provider was added with the first time it is needed, and kept for later
/// requests. A provider that was removed and added again with other settings gets a new adapter.
async fn added_provider_adapter<C: CommandOperation>(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
    operation: &C,
) -> Result<ProviderAdapter, TestConnectionError> {
    let id = provider_type.to_string();
    authorize_command(state.authorization_checker.as_ref(), caller.clone(), &id, operation)
        .await
        .map_err(TestConnectionError::Authorization)?;

    let mut providers = get_key_providers(state, caller).await?;
    if !providers.providers.contains_key(&provider_type) {
        return Err(TestConnectionError::NotFound(provider_type));
    }
    let settings = providers.settings.shift_remove(&provider_type);
    adapter(state, provider_type, settings).ok_or(TestConnectionError::Unsupported(provider_type))
}

/// The adapter of the provider of `provider_type` added with `settings`, built on first use and built again when the
/// provider was added again with other settings. `None` when this instance can't use providers of this type.
fn adapter(
    state: &KmsState,
    provider_type: KeyProviderType,
    settings: Option<KeyProviderSettings>,
) -> Option<ProviderAdapter> {
    let factory = state.factories.get(&provider_type)?;

    let mut built = state.built.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = built
        .get(&provider_type)
        .filter(|existing| existing.settings == settings)
    {
        return Some(Ok(existing.adapter.clone()));
    }
    let adapter = factory.build(settings.as_ref());
    if let Ok(adapter) = &adapter {
        built.insert(
            provider_type,
            BuiltAdapter {
                settings,
                adapter: adapter.clone(),
            },
        );
    }
    Some(adapter)
}

#[derive(Debug, Error)]
pub enum CreateKeyError {
    #[error(transparent)]
    Provider(#[from] TestConnectionError),
    #[error(transparent)]
    Unreachable(#[from] ConnectionError),
    #[error("Keys that sign with {0} can't be created")]
    UnsupportedAlgorithm(JwsAlgorithm),
    #[error(transparent)]
    InvalidLabel(#[from] InvalidLabel),
    #[error(transparent)]
    KeyStore(#[from] KeyStoreError),
    #[error(transparent)]
    Record(#[from] CommandHandlerError<KeyError>),
}

/// Creates a key in an added provider and records its reference. Returns the new key's ID and its reference.
///
/// The key is generated before it is recorded. When it can't be recorded, it is deleted from the provider again, which
/// also frees its label. Once the key is being generated, a caller that gives up doesn't stop this.
///
/// # Errors
///
/// Fails when the algorithm isn't supported, the caller isn't authorized, the provider hasn't been added or has no
/// adapter, the provider can't give the key its label, or the provider can't create the key.
pub async fn create_key(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
    algorithm: JwsAlgorithm,
    label: Option<String>,
) -> Result<(String, KeyReference), CreateKeyError> {
    let key_type = match algorithm {
        JwsAlgorithm::ES256 => KeyType::P256,
        JwsAlgorithm::EdDSA => KeyType::Ed25519,
        algorithm => return Err(CreateKeyError::UnsupportedAlgorithm(algorithm)),
    };
    let adapter = added_provider_adapter(state, caller.clone(), provider_type, &CreateKey).await??;
    if let Some(label) = &label {
        adapter.check_label(label)?;
    }
    // Recording the key is authorized on its own id, which a checker can decide on differently than on the provider.
    // It is checked before the key is created, so a refusal doesn't create a key only to delete it again.
    let id = Uuid::new_v4().to_string();
    authorize_command(state.authorization_checker.as_ref(), caller.clone(), &id, &CreateKey)
        .await
        .map_err(|error| CreateKeyError::Record(CommandHandlerError::Authorization(error)))?;

    let state = state.clone();
    let spec = KeySpec::new(key_type, algorithm);
    tokio::spawn(async move {
        let spec = match &label {
            Some(label) => spec.with_label(label),
            None => spec,
        };
        let provider_key_id = adapter.create(spec).await?;

        let recorded = record_created_key(
            &state,
            caller,
            id,
            adapter.as_ref(),
            provider_type,
            &provider_key_id,
            label.clone(),
            algorithm,
        )
        .await;
        if recorded.is_err() {
            if let Err(error) = adapter.discard(&provider_key_id, label.as_deref()).await {
                let key = &provider_key_id;
                error!(%key, %error, "A key was created in the `{provider_type}` key provider but neither recorded nor deleted");
            }
        }
        recorded
    })
    .await
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

#[allow(clippy::too_many_arguments)]
async fn record_created_key(
    state: &KmsState,
    caller: Caller,
    id: String,
    adapter: &dyn KeyProviderAdapter,
    provider_type: KeyProviderType,
    provider_key_id: &KeyId,
    label: Option<String>,
    algorithm: JwsAlgorithm,
) -> Result<(String, KeyReference), CreateKeyError> {
    let reference = KeyReference {
        provider_type,
        public_key: adapter.public_jwk(provider_key_id).await?,
        provider_key_id: provider_key_id.as_str().to_string(),
        label,
        algorithm,
    };

    command_handler(
        state.authorization_checker.clone(),
        caller,
        &id,
        &state.command.keys,
        KeyCommand::RecordCreatedKey {
            reference: reference.clone(),
        },
    )
    .await?;

    Ok((id, reference))
}

/// Returns every key this instance created, by key ID, in the order they were created.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the keys can't be loaded.
pub async fn get_keys(state: &KmsState, caller: Caller) -> Result<AllKeysView, QueryHandlerError> {
    Ok(query_handler(
        state.authorization_checker.clone(),
        caller,
        "all_keys",
        None,
        &state.query.all_keys,
    )
    .await?
    .unwrap_or_default())
}

/// A key this instance created, with the adapter of its provider, which signs with it.
#[derive(Clone)]
pub struct SigningKey {
    pub reference: KeyReference,
    pub adapter: Arc<dyn KeyProviderAdapter>,
}

/// Returns the key `key_id` to sign with, or `None` when there is no such key or this instance has no adapter for its
/// provider.
///
/// # Errors
///
/// Fails when the key or the providers can't be loaded.
pub async fn signing_key(state: &KmsState, key_id: &str) -> Result<Option<SigningKey>, PersistenceError> {
    let Some(KeyView {
        reference: Some(reference),
    }) = public_query_handler(key_id, &state.query.key).await?
    else {
        return Ok(None);
    };
    let providers = providers(state).await?;
    Ok(with_adapter(state, &providers, reference))
}

/// Returns every key this instance created and has an adapter for, to sign with.
///
/// # Errors
///
/// Fails when the keys or the providers can't be loaded.
pub async fn signing_keys(state: &KmsState) -> Result<Vec<SigningKey>, PersistenceError> {
    let providers = providers(state).await?;
    Ok(public_query_handler("all_keys", &state.query.all_keys)
        .await?
        .unwrap_or_default()
        .keys
        .into_values()
        .filter_map(|reference| with_adapter(state, &providers, reference))
        .collect())
}

async fn providers(state: &KmsState) -> Result<KeyProvidersView, PersistenceError> {
    Ok(public_query_handler(KeyProviders::ID, &state.query.key_providers)
        .await?
        .unwrap_or_default())
}

/// The key with the adapter of its provider, built from the settings the provider was added with. `None` when the
/// adapter can't be built.
fn with_adapter(state: &KmsState, providers: &KeyProvidersView, reference: KeyReference) -> Option<SigningKey> {
    let settings = providers.settings.get(&reference.provider_type).cloned();
    let adapter = adapter(state, reference.provider_type, settings)?.ok()?;
    Some(SigningKey { reference, adapter })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::test_utils::UnreachableKeyProvider;
    use crate::adapter::{FixedAdapter, InMemoryKeyStore, KeyProviderFactory};
    use crate::key_provider::command::KeyProviderCommand;
    use crate::key_provider::settings::{StackitKmsSettings, StackitRegion};
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use identity_kit::{
        jose::Jwk,
        keys::{KeyStore, KeyStoreCapabilities},
    };
    use shared_kernel::authorization::{AuthorizationOperation, AuthorizationRequest};
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    async fn state_with(factories: KeyProviderFactories) -> KmsState {
        kms_state(&InMemory, &EventBusHandle::default(), true, factories).await
    }

    fn fixed(adapter: Arc<dyn KeyProviderAdapter>) -> Arc<dyn KeyProviderFactory> {
        Arc::new(FixedAdapter(adapter))
    }

    /// Only AWS has an adapter, and it is unreachable, so an added in-memory provider is unsupported.
    async fn state() -> KmsState {
        state_with([(KeyProviderType::AwsKmsAmbient, fixed(Arc::new(UnreachableKeyProvider)))].into()).await
    }

    /// An in-memory provider that can be made slow to generate, unable to return public keys, or refusing labels. It
    /// counts the keys it generated.
    struct FakeProvider {
        keys: InMemoryKeyStore,
        generate_delay: Duration,
        public_keys_fail: bool,
        labels_refused: bool,
        generated: AtomicUsize,
    }

    impl FakeProvider {
        fn new() -> Self {
            Self {
                keys: InMemoryKeyStore::new(),
                generate_delay: Duration::ZERO,
                public_keys_fail: false,
                labels_refused: false,
                generated: AtomicUsize::new(0),
            }
        }
    }

    /// Allows commands only on the in-memory provider, as a checker that decides per resource could.
    struct ProviderOnly;

    #[async_trait]
    impl AuthorizationChecker for ProviderOnly {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            match &request.operation {
                AuthorizationOperation::Command { aggregate_id, .. } if aggregate_id != "in_memory" => {
                    Err(AuthorizationError::Forbidden)
                }
                _ => Ok(()),
            }
        }
    }

    #[async_trait]
    impl KeyStore for FakeProvider {
        fn capabilities(&self) -> KeyStoreCapabilities {
            self.keys.capabilities()
        }

        async fn public_jwk(&self, key: &KeyId) -> Result<Jwk, KeyStoreError> {
            if self.public_keys_fail {
                return Err(KeyStoreError::backend(ConnectionError("not reachable".to_string())));
            }
            self.keys.public_jwk(key).await
        }

        async fn sign(&self, key: &KeyId, alg: JwsAlgorithm, data: &[u8]) -> Result<Vec<u8>, KeyStoreError> {
            self.keys.sign(key, alg, data).await
        }

        async fn exists(&self, key: &KeyId) -> Result<bool, KeyStoreError> {
            self.keys.exists(key).await
        }

        async fn generate(&self, spec: KeySpec) -> Result<KeyId, KeyStoreError> {
            tokio::time::sleep(self.generate_delay).await;
            self.generated.fetch_add(1, Ordering::SeqCst);
            self.keys.generate(spec).await
        }

        async fn delete(&self, key: &KeyId) -> Result<(), KeyStoreError> {
            self.keys.delete(key).await
        }
    }

    #[async_trait]
    impl KeyProviderAdapter for FakeProvider {
        async fn test_connection(&self) -> Result<(), ConnectionError> {
            Ok(())
        }

        async fn list_keys(&self) -> Result<ProviderKeys, ConnectionError> {
            Ok(ProviderKeys::default())
        }

        fn check_label(&self, label: &str) -> Result<(), InvalidLabel> {
            if self.labels_refused {
                return Err(InvalidLabel(format!("`{label}` is refused")));
            }
            Ok(())
        }
    }

    /// A state whose in-memory provider is `provider`, added.
    async fn state_with_provider(provider: Arc<FakeProvider>) -> KmsState {
        let state = state_with([(KeyProviderType::InMemory, fixed(provider))].into()).await;
        add(&state, KeyProviderType::InMemory).await;
        state
    }

    async fn create_labelled(state: &KmsState) -> Result<(String, KeyReference), CreateKeyError> {
        create_key(
            state,
            Caller::Internal,
            KeyProviderType::InMemory,
            JwsAlgorithm::EdDSA,
            Some("issuer".to_string()),
        )
        .await
    }

    async fn add(state: &KmsState, provider_type: KeyProviderType) {
        add_with(state, provider_type, None).await;
    }

    async fn add_with(state: &KmsState, provider_type: KeyProviderType, settings: Option<KeyProviderSettings>) {
        change(
            state,
            KeyProviderCommand::AddKeyProvider {
                provider_type,
                name: provider_type.to_string(),
                settings,
            },
        )
        .await;
    }

    async fn change(state: &KmsState, command: KeyProviderCommand) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            command,
        )
        .await
        .unwrap();
    }

    fn stackit_settings(key_ring: u128) -> KeyProviderSettings {
        KeyProviderSettings::StackitKms(StackitKmsSettings {
            project_id: Uuid::from_u128(1),
            region: StackitRegion::Eu01,
            key_ring_id: Uuid::from_u128(key_ring),
            service_account_email: "unicore@sa.stackit.cloud".to_string(),
        })
    }

    /// Builds reachable adapters, or fails to when `unavailable` is set, and records the settings of every build.
    #[derive(Default)]
    struct RecordingFactory {
        builds: Mutex<Vec<Option<KeyProviderSettings>>>,
        unavailable: bool,
    }

    impl RecordingFactory {
        fn builds(&self) -> Vec<Option<KeyProviderSettings>> {
            self.builds.lock().unwrap().clone()
        }
    }

    impl KeyProviderFactory for RecordingFactory {
        fn build(
            &self,
            settings: Option<&KeyProviderSettings>,
        ) -> Result<Arc<dyn KeyProviderAdapter>, ConnectionError> {
            self.builds.lock().unwrap().push(settings.cloned());
            if self.unavailable {
                return Err(ConnectionError("no workload identity token".to_string()));
            }
            Ok(Arc::new(FakeProvider::new()))
        }
    }

    async fn stackit_state(factory: Arc<RecordingFactory>) -> KmsState {
        state_with([(KeyProviderType::StackitKms, factory as Arc<dyn KeyProviderFactory>)].into()).await
    }

    async fn test_stackit(state: &KmsState) -> Result<(), ConnectionError> {
        test_key_provider_connection(state, Caller::Internal, KeyProviderType::StackitKms)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn builds_the_adapter_from_the_settings_the_provider_was_added_with() {
        let factory = Arc::new(RecordingFactory::default());
        let state = stackit_state(factory.clone()).await;
        add_with(&state, KeyProviderType::StackitKms, Some(stackit_settings(2))).await;

        test_stackit(&state).await.unwrap();

        assert_eq!(factory.builds(), vec![Some(stackit_settings(2))]);
    }

    #[tokio::test]
    async fn reuses_the_adapter_it_built() {
        let factory = Arc::new(RecordingFactory::default());
        let state = stackit_state(factory.clone()).await;
        add_with(&state, KeyProviderType::StackitKms, Some(stackit_settings(2))).await;
        test_stackit(&state).await.unwrap();

        test_stackit(&state).await.unwrap();

        assert_eq!(factory.builds().len(), 1);
    }

    #[tokio::test]
    async fn builds_a_new_adapter_for_a_provider_added_again_with_other_settings() {
        let factory = Arc::new(RecordingFactory::default());
        let state = stackit_state(factory.clone()).await;
        add_with(&state, KeyProviderType::StackitKms, Some(stackit_settings(2))).await;
        test_stackit(&state).await.unwrap();
        change(
            &state,
            KeyProviderCommand::RemoveKeyProvider {
                provider_type: KeyProviderType::StackitKms,
            },
        )
        .await;
        add_with(&state, KeyProviderType::StackitKms, Some(stackit_settings(3))).await;

        test_stackit(&state).await.unwrap();

        assert_eq!(factory.builds().last(), Some(&Some(stackit_settings(3))));
    }

    #[tokio::test]
    async fn a_provider_whose_adapter_cannot_be_built_is_reported_unreachable() {
        let state = stackit_state(Arc::new(RecordingFactory {
            unavailable: true,
            ..RecordingFactory::default()
        }))
        .await;
        add_with(&state, KeyProviderType::StackitKms, Some(stackit_settings(2))).await;

        let result = test_stackit(&state).await;

        assert_eq!(result.unwrap_err().to_string(), "no workload identity token");
    }

    #[tokio::test]
    async fn leaves_out_a_development_provider_outside_the_development_profile() {
        let mut state = state().await;
        add(&state, KeyProviderType::InMemory).await;
        state.development_providers_allowed = false;

        let providers = get_key_providers(&state, Caller::Internal).await.unwrap();

        assert!(providers.providers.is_empty());
    }

    #[tokio::test]
    async fn keeps_a_production_provider_outside_the_development_profile() {
        let mut state = state().await;
        add(&state, KeyProviderType::InMemory).await;
        add(&state, KeyProviderType::AwsKmsAmbient).await;
        state.development_providers_allowed = false;

        let providers = get_key_providers(&state, Caller::Internal).await.unwrap();

        assert!(!providers.providers.contains_key(&KeyProviderType::InMemory));
        assert!(providers.providers.contains_key(&KeyProviderType::AwsKmsAmbient));
    }

    #[tokio::test]
    async fn an_unreachable_provider_is_reported_not_failed() {
        let state = state().await;
        add(&state, KeyProviderType::AwsKmsAmbient).await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::AwsKmsAmbient).await;

        assert_eq!(result.unwrap().unwrap_err().to_string(), "connection refused");
    }

    #[tokio::test]
    async fn a_provider_that_was_not_added_is_not_found() {
        let state = state().await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::AwsKmsAmbient).await;

        assert!(matches!(
            result,
            Err(TestConnectionError::NotFound(KeyProviderType::AwsKmsAmbient))
        ));
    }

    #[tokio::test]
    async fn a_provider_without_an_adapter_is_unsupported() {
        let state = state().await;
        add(&state, KeyProviderType::InMemory).await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::InMemory).await;

        assert!(matches!(
            result,
            Err(TestConnectionError::Unsupported(KeyProviderType::InMemory))
        ));
    }

    #[tokio::test]
    async fn records_the_public_key_the_provider_holds() {
        let provider = Arc::new(FakeProvider::new());
        let state = state_with_provider(provider.clone()).await;

        let (_, reference) = create_labelled(&state).await.unwrap();

        let held = provider
            .public_jwk(&KeyId::new(reference.provider_key_id))
            .await
            .unwrap();
        assert_eq!(reference.public_key, held);
    }

    #[tokio::test]
    async fn refuses_a_label_the_provider_cannot_give() {
        let state = state_with_provider(Arc::new(FakeProvider {
            labels_refused: true,
            ..FakeProvider::new()
        }))
        .await;

        let result = create_labelled(&state).await;

        assert_eq!(result.unwrap_err().to_string(), "`issuer` is refused");
    }

    #[tokio::test]
    async fn deletes_a_key_it_could_not_record() {
        let provider = Arc::new(FakeProvider {
            public_keys_fail: true,
            ..FakeProvider::new()
        });
        let state = state_with_provider(provider.clone()).await;
        create_labelled(&state).await.unwrap_err();

        let left = provider.exists(&KeyId::new("issuer")).await.unwrap();

        assert!(!left);
    }

    #[tokio::test]
    async fn creates_no_key_that_it_may_not_record() {
        let provider = Arc::new(FakeProvider::new());
        let mut state = state_with_provider(provider.clone()).await;
        state.authorization_checker = Arc::new(ProviderOnly);

        let result = create_labelled(&state).await;

        assert!(matches!(
            result,
            Err(CreateKeyError::Record(CommandHandlerError::Authorization(_)))
        ));
        assert_eq!(provider.generated.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn records_the_key_although_the_caller_gives_up() {
        let state = state_with_provider(Arc::new(FakeProvider {
            generate_delay: Duration::from_millis(100),
            ..FakeProvider::new()
        }))
        .await;
        tokio::time::timeout(Duration::from_millis(20), create_labelled(&state))
            .await
            .unwrap_err();
        tokio::time::sleep(Duration::from_millis(300)).await;

        let keys = get_keys(&state, Caller::Internal).await.unwrap().keys;

        assert_eq!(keys.len(), 1);
    }

    #[tokio::test]
    async fn refuses_an_algorithm_it_does_not_create_keys_for() {
        let state = state().await;
        add(&state, KeyProviderType::AwsKmsAmbient).await;

        let result = create_key(
            &state,
            Caller::Internal,
            KeyProviderType::AwsKmsAmbient,
            JwsAlgorithm::ES384,
            None,
        )
        .await;

        assert!(matches!(
            result,
            Err(CreateKeyError::UnsupportedAlgorithm(JwsAlgorithm::ES384))
        ));
    }

    #[tokio::test]
    async fn records_nothing_when_the_provider_fails() {
        let state = state().await;
        add(&state, KeyProviderType::AwsKmsAmbient).await;

        let _ = create_key(
            &state,
            Caller::Internal,
            KeyProviderType::AwsKmsAmbient,
            JwsAlgorithm::ES256,
            None,
        )
        .await;

        assert!(get_keys(&state, Caller::Internal).await.unwrap().keys.is_empty());
    }

    #[tokio::test]
    async fn gives_a_key_to_sign_with_with_the_adapter_of_its_provider() {
        let state = state_with_provider(Arc::new(FakeProvider::new())).await;
        let (id, reference) = create_labelled(&state).await.unwrap();

        let signing_key = signing_key(&state, &id).await.unwrap();

        assert_eq!(signing_key.map(|key| key.reference), Some(reference));
    }

    #[tokio::test]
    async fn an_unknown_key_cannot_sign() {
        let state = state_with_provider(Arc::new(FakeProvider::new())).await;

        let signing_key = signing_key(&state, "unknown").await.unwrap();

        assert!(signing_key.is_none());
    }

    #[tokio::test]
    async fn a_key_without_an_adapter_for_its_provider_cannot_sign() {
        let mut state = state_with_provider(Arc::new(FakeProvider::new())).await;
        let (id, _) = create_labelled(&state).await.unwrap();
        state.factories = Arc::new(KeyProviderFactories::new());

        let signing_key = signing_key(&state, &id).await.unwrap();

        assert!(signing_key.is_none());
    }

    #[tokio::test]
    async fn gives_every_created_key_to_sign_with() {
        let state = state_with_provider(Arc::new(FakeProvider::new())).await;
        let (_, reference) = create_labelled(&state).await.unwrap();

        let signing_keys = signing_keys(&state).await.unwrap();

        assert_eq!(
            signing_keys.into_iter().map(|key| key.reference).collect::<Vec<_>>(),
            vec![reference]
        );
    }
}
