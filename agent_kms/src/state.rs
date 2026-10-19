use crate::adapter::{ConnectionError, KeyProviderAdapters};
use crate::key_provider::{
    aggregate::{KeyProvider, KeyProviderType},
    command::TestKeyProviderConnection,
    services::KeyProviderServices,
    views::{all_key_providers::AllKeyProvidersView, KeyProviderView},
};
use agent_shared::{
    application_state::{CommandHandler, CqrsComponentBuilder},
    handlers::{authorize_command, live_query_handler, QueryHandlerError},
};
use shared_kernel::authorization::{
    AllowAllAuthorizationChecker, AuthorizationChecker, AuthorizationError, Caller, QueryOperation,
};
use shared_kernel::event_bus::EventBusHandle;
use shared_kernel::view_repository::DynViewRepository;
use std::sync::Arc;
use thiserror::Error;

impl QueryOperation for KeyProviderView {
    const OPERATION_NAME: &'static str = "kms.key_providers.get";
}

impl QueryOperation for AllKeyProvidersView {
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
    pub key_provider: CommandHandler<KeyProvider>,
}

#[derive(Clone)]
pub struct Queries {
    pub key_provider: Arc<dyn DynViewRepository<KeyProviderView, KeyProvider>>,
    pub all_key_providers: Arc<dyn DynViewRepository<AllKeyProvidersView, KeyProvider>>,
}

/// Constructs the CQRS components of the KMS bounded context.
///
/// `adapters` holds the adapter for every provider type this instance can connect to.
///
/// # Panics
///
/// Panics if the key provider services were already given a view repository, which would be a wiring bug.
pub async fn kms_state<CCB: CqrsComponentBuilder>(
    builder: &CCB,
    event_bus: &EventBusHandle,
    development_providers_allowed: bool,
    adapters: KeyProviderAdapters,
) -> KmsState {
    let services = Arc::new(KeyProviderServices::new(development_providers_allowed));

    let (key_provider_command_handler, key_provider, all_key_providers) = builder
        .commands_and_queries::<KeyProviderView, KeyProvider, AllKeyProvidersView>(
            services.clone(),
            vec![event_bus.query()],
        )
        .await;

    assert!(
        services.all_key_providers.set(all_key_providers.clone()).is_ok(),
        "key provider view already initialized"
    );

    KmsState {
        authorization_checker: Arc::new(AllowAllAuthorizationChecker),
        command: CommandHandlers {
            key_provider: key_provider_command_handler,
        },
        query: Queries {
            key_provider,
            all_key_providers,
        },
        adapters: Arc::new(adapters),
    }
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

/// Returns the provider of the given type, or `None` if it hasn't been added or has been removed.
///
/// # Errors
///
/// Fails when the caller isn't authorized or the provider can't be loaded.
pub async fn get_key_provider(
    state: &KmsState,
    caller: Caller,
    provider_type: KeyProviderType,
) -> Result<Option<KeyProviderView>, QueryHandlerError> {
    let id = provider_type.to_string();
    live_query_handler(
        state.authorization_checker.as_ref(),
        caller,
        &id,
        Some(&id),
        state.query.key_provider.as_ref(),
    )
    .await
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
    let id = provider_type.to_string();
    authorize_command(
        state.authorization_checker.as_ref(),
        caller.clone(),
        &id,
        &TestKeyProviderConnection,
    )
    .await
    .map_err(TestConnectionError::Authorization)?;

    get_key_provider(state, caller, provider_type)
        .await?
        .ok_or(TestConnectionError::NotFound(provider_type))?;

    let adapter = state
        .adapters
        .get(&provider_type)
        .ok_or(TestConnectionError::Unsupported(provider_type))?;

    Ok(adapter.test_connection().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{InMemoryKeyProvider, KeyProviderAdapter};
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

    async fn add(state: &KmsState, provider_type: KeyProviderType, name: &str) -> Result<(), KeyProviderError> {
        command_handler(
            state.authorization_checker.clone(),
            Caller::Internal,
            &provider_type.to_string(),
            &state.command.key_provider,
            KeyProviderCommand::AddKeyProvider {
                provider_type,
                name: name.to_string(),
            },
        )
        .await
        .map_err(|error| match error {
            CommandHandlerError::Aggregate(AggregateError::UserError(error)) => error,
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

        let all = state
            .query
            .all_key_providers
            .load(AllKeyProvidersView::VIEW_ID)
            .await
            .unwrap()
            .unwrap();
        let names: Vec<_> = all
            .key_providers
            .values()
            .map(|provider| provider.name.as_str())
            .collect();
        assert_eq!(names, ["Local keys", "Production keys"]);
    }

    #[tokio::test]
    async fn provider_names_are_unique_across_types() {
        let state = state().await;

        add(&state, KeyProviderType::InMemory, "Keys").await.unwrap();

        assert_eq!(
            add(&state, KeyProviderType::AwsKmsAmbient, "Keys").await,
            Err(KeyProviderError::NameTaken("Keys".to_string()))
        );
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
