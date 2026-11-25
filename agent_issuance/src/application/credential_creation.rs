use crate::{
    credential::{command::CredentialCommand, error::CredentialError},
    refresh::{command::RefreshCapabilityCommand, token::RefreshToken},
    state::IssuanceState,
};
use agent_shared::handlers::{command_handler, public_command_handler, CommandHandlerError};
use shared_kernel::authorization::Caller;
use tracing::error;

/// Why [`create_credential`] created nothing.
#[derive(Debug, thiserror::Error)]
pub enum CreateCredentialError {
    /// The configuration opts into refresh and the credential's refresh capability could not be created, so the
    /// credential was not created either.
    #[error("Failed to create the refresh capability of the credential")]
    RefreshCapability,
    /// The credential command failed.
    #[error(transparent)]
    Credential(CommandHandlerError<CredentialError>),
}

/// Executes `command`, which creates the credential `credential_id`, as `caller`.
///
/// An unsigned credential whose configuration opts into refresh first gets a new [`RefreshToken`] and the refresh
/// capability it unlocks, and then embeds the token. The capability is created first, so a refreshable credential
/// never exists without a working token; a capability whose credential then fails is unreachable, since its token
/// was never handed out. Pre-signed credentials can't be changed, so they get no refresh capability.
pub async fn create_credential(
    state: &IssuanceState,
    caller: Caller,
    credential_id: &str,
    credential_configuration_id: &str,
    command: CredentialCommand,
) -> Result<(), CreateCredentialError> {
    let command = match command {
        CredentialCommand::CreateUnsignedCredential {
            credential_id: _,
            data,
            credential_configuration,
            expires_at,
            refresh_token: _,
        } if credential_configuration.refresh_service.is_some() => {
            let refresh_token = RefreshToken::generate();
            public_command_handler(
                &refresh_token.capability_id(),
                &state.command.refresh_capability,
                RefreshCapabilityCommand::CreateRefreshCapability {
                    credential_id: credential_id.to_owned(),
                    credential_configuration_id: credential_configuration_id.to_owned(),
                },
            )
            .await
            .map_err(|failure| {
                error!(error = %failure, credential_id, "Failed to create the refresh capability of a credential");
                CreateCredentialError::RefreshCapability
            })?;
            CredentialCommand::CreateUnsignedCredential {
                credential_id: credential_id.to_owned(),
                data,
                credential_configuration,
                expires_at,
                refresh_token: Some(refresh_token),
            }
        }
        command => command,
    };

    command_handler(
        state.authorization_checker.clone(),
        caller,
        credential_id,
        &state.command.credential,
        command,
    )
    .await
    .map_err(CreateCredentialError::Credential)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        credential::{
            aggregate::{test_utils::DC_SD_JWT_CREDENTIAL_CONFIGURATION, CredentialExpiry},
            entity::Data,
        },
        refresh::{aggregate::RefreshCapability, error::RefreshCapabilityError},
        server_config::supported_configuration::SupportedCredentialConfiguration,
        services::IssuanceServices,
        state::issuance_state,
    };
    use agent_secret_manager::service::Service;
    use agent_shared::{
        application_state::Command, config::RefreshServiceConfiguration, handlers::public_query_handler,
    };
    use agent_store::in_memory::InMemory;
    use cqrs_es::AggregateError;
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc};

    fn create_opted_in_credential() -> CredentialCommand {
        CredentialCommand::CreateUnsignedCredential {
            credential_id: "credential".to_owned(),
            data: Data {
                raw: json!({"first_name": "Ferris"}),
            },
            credential_configuration: Box::new(SupportedCredentialConfiguration {
                metadata: DC_SD_JWT_CREDENTIAL_CONFIGURATION.clone(),
                refresh_service: Some(RefreshServiceConfiguration {
                    type_: "VerifiableCredentialRefreshService2021".to_owned(),
                }),
            }),
            expires_at: CredentialExpiry::Never,
            refresh_token: None,
        }
    }

    async fn create(state: &IssuanceState) -> Result<(), CreateCredentialError> {
        create_credential(
            state,
            Caller::Internal,
            "credential",
            "configuration",
            create_opted_in_credential(),
        )
        .await
    }

    #[tokio::test]
    async fn issuing_an_opted_in_credential_creates_its_capability() {
        let state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;

        create(&state).await.unwrap();

        let credential = public_query_handler("credential", &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let token = RefreshToken::from(
            credential.data.unwrap().raw["refreshService"]["refreshToken"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
        let capability = public_query_handler(&token.capability_id(), &state.query.refresh_capability)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                capability.credential_id.as_str(),
                capability.credential_configuration_id.as_str()
            ),
            ("credential", "configuration")
        );
    }

    /// An event store that refuses every refresh capability command.
    struct UnavailableStore;

    #[async_trait::async_trait]
    impl Command<RefreshCapability> for UnavailableStore {
        async fn execute_with_metadata(
            &self,
            _aggregate_id: &str,
            _command: RefreshCapabilityCommand,
            _metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<RefreshCapabilityError>> {
            Err(AggregateError::DatabaseConnectionError("unavailable".into()))
        }
    }

    #[tokio::test]
    async fn failing_to_create_a_capability_fails_the_issuance() {
        let mut state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        state.command.refresh_capability = Arc::new(UnavailableStore);

        let result = create(&state).await;

        assert!(matches!(result, Err(CreateCredentialError::RefreshCapability)));
        assert!(public_query_handler("credential", &state.query.credential)
            .await
            .unwrap()
            .is_none());
    }
}
