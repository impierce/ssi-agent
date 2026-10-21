use crate::adapter::{ConnectionError, KeyProviderAdapter, KeyProviderAdapters, ProviderKey};
use crate::key_provider::{
    aggregate::{KeyProviderType, KeyProviders},
    command::{ListKeyProviderKeys, TestKeyProviderConnection},
    services::KeyProviderServices,
    views::KeyProvidersView,
};
use agent_shared::{
    application_state::{CommandHandler, CqrsComponentBuilder},
    handlers::{authorize_command, query_handler, QueryHandlerError},
};
use shared_kernel::authorization::{
    AllowAllAuthorizationChecker, AuthorizationChecker, AuthorizationError, Caller, CommandOperation, QueryOperation,
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
}

#[derive(Clone)]
pub struct CommandHandlers {
    pub key_providers: CommandHandler<KeyProviders>,
}

#[derive(Clone)]
pub struct Queries {
    pub key_providers: Arc<dyn DynViewRepository<KeyProvidersView, KeyProviders>>,
}

/// Constructs the CQRS components of the KMS bounded context.
///
/// `adapters` holds the adapter for every provider type this instance can connect to.
pub async fn kms_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_bus: &EventBusHandle,
    development_providers_allowed: bool,
    adapters: KeyProviderAdapters,
) -> KmsState {
    let services = Arc::new(KeyProviderServices::new(development_providers_allowed));

    // There is only one key providers aggregate, so its own view already lists every provider and the
    // all-aggregates view is not needed.
    let (key_providers_command_handler, key_providers, _) = builder
        .commands_and_queries::<KeyProvidersView, KeyProviders, KeyProvidersView>(services, vec![event_bus.query()])
        .await;

    KmsState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: CommandHandlers {
            key_providers: key_providers_command_handler,
        },
        query: Queries { key_providers },
        adapters: Arc::new(adapters),
    }
}

/// Returns every key provider this instance is connected to, by type, in the order they were added.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the providers can't be loaded.
pub async fn get_key_providers(state: &KmsState, caller: Caller) -> Result<KeyProvidersView, QueryHandlerError> {
    Ok(query_handler(
        state.authorization_checker.clone(),
        caller,
        KeyProviders::ID,
        None,
        &state.query.key_providers,
    )
    .await?
    .unwrap_or_default())
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
    let adapter = added_provider_adapter(state, caller, provider_type, &TestKeyProviderConnection).await?;
    Ok(adapter.test_connection().await)
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
) -> Result<Result<Vec<ProviderKey>, ConnectionError>, TestConnectionError> {
    let adapter = added_provider_adapter(state, caller, provider_type, &ListKeyProviderKeys).await?;
    Ok(adapter.list_keys().await)
}

/// The adapter of an added provider, after checking that the caller may perform `operation` on it.
async fn added_provider_adapter<C: CommandOperation>(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
    operation: &C,
) -> Result<Arc<dyn KeyProviderAdapter>, TestConnectionError> {
    let id = provider_type.to_string();
    authorize_command(state.authorization_checker.as_ref(), caller.clone(), &id, operation)
        .await
        .map_err(TestConnectionError::Authorization)?;

    get_key_provider(state, caller, provider_type)
        .await?
        .ok_or(TestConnectionError::NotFound(provider_type))?;

    state
        .adapters
        .get(&provider_type)
        .cloned()
        .ok_or(TestConnectionError::Unsupported(provider_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::InMemoryKeyProvider;
    use crate::key_provider::{command::KeyProviderCommand, error::KeyProviderError};
    use agent_shared::handlers::{command_handler, CommandHandlerError};
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use cqrs_es::AggregateError;

    struct Unreachable;

    #[async_trait]
    impl KeyProviderAdapter for Unreachable {
        async fn test_connection(&self) -> Result<(), ConnectionError> {
            Err(ConnectionError("connection refused".to_string()))
        }

        async fn list_keys(&self) -> Result<Vec<crate::adapter::ProviderKey>, ConnectionError> {
            Err(ConnectionError("connection refused".to_string()))
        }
    }

    async fn state() -> KmsState {
        let adapters: KeyProviderAdapters = [
            (
                KeyProviderType::InMemory,
                Arc::new(InMemoryKeyProvider) as Arc<dyn KeyProviderAdapter>,
            ),
            (KeyProviderType::AwsKmsAmbient, Arc::new(Unreachable)),
        ]
        .into();
        kms_state(&InMemory, &EventBusHandle::default(), true, adapters).await
    }

    /// Adds a provider. A rejected command is `Err(Some(error))`; one that lost a concurrent write is `Err(None)`.
    async fn add(state: &KmsState, provider_type: KeyProviderType, name: &str) -> Result<(), Option<KeyProviderError>> {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            KeyProviders::ID,
            &state.command.key_providers,
            KeyProviderCommand::AddKeyProvider {
                provider_type,
                name: name.to_string(),
            },
        )
        .await
        .map_err(|error| match error {
            CommandHandlerError::Aggregate(AggregateError::UserError(error)) => Some(error),
            CommandHandlerError::Aggregate(AggregateError::AggregateConflict) => None,
            error => panic!("unexpected error: {error}"),
        })
    }

    #[tokio::test]
    async fn providers_of_different_types_run_side_by_side() {
        let state = state().await;

        add(&state, KeyProviderType::InMemory, "Local keys").await.unwrap();
        add(&state, KeyProviderType::AwsKmsAmbient, "Production keys")
            .await
            .unwrap();

        let all = get_key_providers(&state, Caller::Internal).await.unwrap();
        let names: Vec<_> = all.providers.values().map(String::as_str).collect();
        assert_eq!(names, ["Local keys", "Production keys"]);
    }

    #[tokio::test]
    async fn provider_names_are_unique_across_types() {
        let state = state().await;

        add(&state, KeyProviderType::InMemory, "Keys").await.unwrap();

        assert_eq!(
            add(&state, KeyProviderType::AwsKmsAmbient, "Keys").await,
            Err(Some(KeyProviderError::NameTaken("Keys".to_string())))
        );
    }

    #[tokio::test]
    async fn concurrent_additions_cannot_both_take_the_same_name() {
        for _ in 0..50 {
            let state = state().await;

            let (in_memory, aws) = tokio::join!(
                add(&state, KeyProviderType::InMemory, "Keys"),
                add(&state, KeyProviderType::AwsKmsAmbient, "Keys"),
            );

            assert!(in_memory.is_ok() != aws.is_ok(), "{in_memory:?} {aws:?}");
            assert_eq!(
                get_key_providers(&state, Caller::Internal)
                    .await
                    .unwrap()
                    .providers
                    .len(),
                1
            );
        }
    }

    #[tokio::test]
    async fn reports_whether_an_added_provider_is_reachable() {
        let state = state().await;
        add(&state, KeyProviderType::InMemory, "Local keys").await.unwrap();
        add(&state, KeyProviderType::AwsKmsAmbient, "Production keys")
            .await
            .unwrap();

        let reachable = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::InMemory)
            .await
            .unwrap();
        assert!(reachable.is_ok());

        let unreachable = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::AwsKmsAmbient)
            .await
            .unwrap();
        assert_eq!(unreachable.unwrap_err().to_string(), "connection refused");
    }

    #[tokio::test]
    async fn refuses_to_test_a_provider_that_was_not_added() {
        let state = state().await;

        let result = test_key_provider_connection(&state, Caller::Internal, KeyProviderType::AwsKmsAmbient).await;

        assert!(matches!(
            result,
            Err(TestConnectionError::NotFound(KeyProviderType::AwsKmsAmbient))
        ));
    }
}
