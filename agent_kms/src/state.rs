use crate::adapter::{ConnectionError, KeyProviderAdapter, KeyProviderAdapters, ProviderKeys};
use crate::key::{
    aggregate::Key,
    usage::KeyUsage,
    views::{AllKeysView, KeyView},
};
use crate::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    command::{KeyProviderCommand, ListKeyProviderKeys, TestKeyProviderConnection},
    error::KeyProviderError,
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
use shared_kernel::authorization::{
    AllowAllAuthorizationChecker, AuthorizationChecker, AuthorizationError, Caller, QueryOperation,
};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;
use thiserror::Error;

impl QueryOperation for KeyProvidersView {
    const OPERATION_NAME: &'static str = "kms.key_providers.list";
}

#[derive(Clone)]
pub struct KmsState {
    pub authorization_checker: Arc<dyn AuthorizationChecker>,
    pub command: CommandHandlers,
    pub query: Queries,
    pub adapters: Arc<KeyProviderAdapters>,
    /// Whether development-only providers, such as the in-memory provider, may be used.
    pub development_providers_allowed: bool,
    /// What signs with the keys, asked before a key is deleted.
    pub key_usage: Arc<dyn KeyUsage>,
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
/// `adapters` holds the adapters, or the factories that build them, for every provider type this instance can connect
/// to. `key_usage` tells what signs with a key, so that no such key is deleted.
pub async fn kms_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_bus: &EventBusHandle,
    development_providers_allowed: bool,
    adapters: KeyProviderAdapters,
    key_usage: Arc<dyn KeyUsage>,
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
        adapters: Arc::new(adapters),
        development_providers_allowed,
        key_usage,
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

/// Why an added key provider could not be used.
#[derive(Debug, Error)]
pub enum KeyProviderAccessError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Query(#[from] QueryHandlerError),
    #[error("No `{0}` key provider has been added")]
    NotFound(KeyProviderType),
    #[error("This instance has no adapter for `{0}` key providers")]
    Unsupported(KeyProviderType),
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
) -> Result<Result<(), ConnectionError>, KeyProviderAccessError> {
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &provider_type.to_string(),
        &TestKeyProviderConnection,
    )
    .await
    .map_err(KeyProviderAccessError::Authorization)?;

    let provider = added_provider(state, caller, provider_type).await?;
    Ok(provider.adapter.test_connection().await)
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
) -> Result<Result<ProviderKeys, ConnectionError>, KeyProviderAccessError> {
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &provider_type.to_string(),
        &ListKeyProviderKeys,
    )
    .await
    .map_err(KeyProviderAccessError::Authorization)?;

    let provider = added_provider(state, caller, provider_type).await?;
    Ok(provider.adapter.list_keys().await)
}

/// An added provider: the settings it was added with, and the adapter built from them.
pub(crate) struct AddedProvider {
    pub(crate) settings: Option<KeyProviderSettings>,
    pub(crate) adapter: Arc<dyn KeyProviderAdapter>,
}

/// The added provider of `provider_type`. Whether the caller may use it for an operation is checked by the caller.
///
/// # Errors
///
/// Fails when the providers can't be loaded, the provider hasn't been added, or this instance has no adapter for it.
pub(crate) async fn added_provider(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<AddedProvider, KeyProviderAccessError> {
    let mut providers = get_key_providers(state, caller).await?;
    if !providers.providers.contains_key(&provider_type) {
        return Err(KeyProviderAccessError::NotFound(provider_type));
    }
    let settings = providers.settings.shift_remove(&provider_type);
    let Some(adapter) = state.adapters.get(provider_type, settings.as_ref()) else {
        return Err(KeyProviderAccessError::Unsupported(provider_type));
    };
    Ok(AddedProvider { settings, adapter })
}

/// Every key that is recorded and not deleted. Read for the rules of the KMS itself, so it isn't authorized.
pub(crate) async fn all_keys(state: &KmsState) -> Result<AllKeysView, PersistenceError> {
    Ok(public_query_handler(AllKeysView::ID, &state.query.all_keys)
        .await?
        .unwrap_or_default())
}

/// Why a key provider could not be removed.
#[derive(Debug, Error)]
pub enum RemoveKeyProviderError {
    #[error(transparent)]
    Authorization(AuthorizationError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    /// A recorded key that isn't deleted lives in the provider.
    #[error("The `{0}` key provider still has keys; delete them first")]
    HasKeys(KeyProviderType),
    #[error(transparent)]
    Command(#[from] CommandHandlerError<KeyProviderError>),
}

/// Disconnects this instance from a key provider. A provider that still has keys is kept, so that no recorded key
/// loses the provider that holds it.
///
/// # Errors
///
/// Fails when the caller isn't authorized, the provider still has keys, or the provider can't be removed.
pub async fn remove_key_provider(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<(), RemoveKeyProviderError> {
    let command = KeyProviderCommand::RemoveKeyProvider { provider_type };
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        KeyProviders::ID,
        &command,
    )
    .await
    .map_err(RemoveKeyProviderError::Authorization)?;

    if all_keys(state)
        .await?
        .keys
        .values()
        .any(|reference| reference.provider_type == provider_type)
    {
        return Err(RemoveKeyProviderError::HasKeys(provider_type));
    }

    command_handler(
        state.authorization_checker.clone(),
        caller,
        KeyProviders::ID,
        &state.command.key_providers,
        command,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::adapter::{test_utils::InMemoryFactory, UnreachableKeyProvider};
    use crate::key::{aggregate::tests::reference, command::KeyCommand, usage::Unused};
    use crate::key_provider::settings::tests::stackit;
    use agent_store::in_memory::InMemory;

    /// Only AWS has an adapter, so an added in-memory provider is unsupported.
    async fn state() -> KmsState {
        let adapters: KeyProviderAdapters = [(
            KeyProviderType::AwsKmsAmbient,
            Arc::new(UnreachableKeyProvider("connection refused".to_string())) as Arc<dyn KeyProviderAdapter>,
        )]
        .into();
        kms_state(&InMemory, &EventBusHandle::default(), true, adapters, Arc::new(Unused)).await
    }

    async fn add(state: &KmsState, provider_type: KeyProviderType) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            KeyProviderCommand::AddKeyProvider {
                provider_type,
                name: provider_type.to_string(),
                settings: None,
            },
        )
        .await
        .unwrap();
    }

    /// A state with a STACKIT KMS provider added with `settings`, its adapters built by `factory`.
    pub(crate) async fn stackit_state(factory: &InMemoryFactory, settings: KeyProviderSettings) -> KmsState {
        let adapters = KeyProviderAdapters::default().with_factory(KeyProviderType::StackitKms, factory.clone());
        let state = kms_state(&InMemory, &EventBusHandle::default(), true, adapters, Arc::new(Unused)).await;
        add_stackit(&state, settings).await;
        state
    }

    /// Adds the STACKIT KMS provider with `settings`.
    pub(crate) async fn add_stackit(state: &KmsState, settings: KeyProviderSettings) {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            KeyProviderCommand::AddKeyProvider {
                provider_type: KeyProviderType::StackitKms,
                name: "Customer keys".to_string(),
                settings: Some(settings),
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn builds_the_adapter_from_the_settings_the_provider_was_added_with() {
        let factory = InMemoryFactory::default();
        let state = stackit_state(&factory, stackit("key-ring")).await;

        test_key_provider_connection(&state, Caller::Internal, KeyProviderType::StackitKms)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(*factory.builds.lock().unwrap(), [stackit("key-ring")]);
    }

    #[tokio::test]
    async fn a_provider_whose_adapter_cannot_be_built_is_reported_unreachable() {
        let factory = InMemoryFactory {
            failure: Some("no workload identity token".to_string()),
            ..InMemoryFactory::default()
        };
        let state = stackit_state(&factory, stackit("key-ring")).await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::StackitKms).await;

        assert_eq!(result.unwrap().unwrap_err().to_string(), "no workload identity token");
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
            Err(KeyProviderAccessError::NotFound(KeyProviderType::AwsKmsAmbient))
        ));
    }

    #[tokio::test]
    async fn refuses_to_remove_a_provider_that_has_keys() {
        let state = state().await;
        add(&state, KeyProviderType::InMemory).await;
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            "key-id",
            &state.command.keys,
            KeyCommand::CreateKey { reference: reference() },
        )
        .await
        .unwrap();

        let result = remove_key_provider(&state, Caller::Internal, KeyProviderType::InMemory).await;

        assert!(matches!(
            result,
            Err(RemoveKeyProviderError::HasKeys(KeyProviderType::InMemory))
        ));
    }

    #[tokio::test]
    async fn a_provider_without_an_adapter_is_unsupported() {
        let state = state().await;
        add(&state, KeyProviderType::InMemory).await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::InMemory).await;

        assert!(matches!(
            result,
            Err(KeyProviderAccessError::Unsupported(KeyProviderType::InMemory))
        ));
    }
}
