use agent_shared::generate_random_string;
use agent_shared::handlers::command_handler;
use agent_shared::{config::RefreshServiceConfiguration, handlers::query_handler};

use crate::refresh_capability::aggregate::RefreshCapabilityStatus;
use crate::{refresh_capability::command::RefreshCapabilityCommand, state::IssuanceState};

#[derive(Debug, PartialEq)]
pub struct CreateRefreshCapabilityResponse {
    pub refresh_reference: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolveRefreshCapabilityResponse {
    pub refresh_reference: String,
    pub credential_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshCapabilityServiceError {
    #[error("Refresh capability was not found")]
    NotFound,
    #[error("Failed to execute command: {0}")]
    Command(String),
    #[error("Failed to query state: {0}")]
    Query(String),
}

#[derive(Default)]
pub struct RefreshCapabilityService;

impl RefreshCapabilityService {
    pub async fn create_for_credential(
        &self,
        state: &IssuanceState,
        credential_id: &str,
        refresh_service: Option<&RefreshServiceConfiguration>,
    ) -> Result<Option<CreateRefreshCapabilityResponse>, RefreshCapabilityServiceError> {
        if refresh_service.is_none() {
            return Ok(None);
        }

        let refresh_reference = generate_random_string();

        let command = RefreshCapabilityCommand::CreateRefreshCapability {
            refresh_reference: refresh_reference.clone(),
            credential_id: credential_id.to_string(),
        };

        command_handler(&refresh_reference, &state.command.refresh_capability, command)
            .await
            .map_err(|err| RefreshCapabilityServiceError::Command(err.to_string()))?;

        Ok(Some(CreateRefreshCapabilityResponse { refresh_reference }))
    }

    pub async fn resolve_active(
        &self,
        state: &IssuanceState,
        refresh_reference: &str,
    ) -> Result<ResolveRefreshCapabilityResponse, RefreshCapabilityServiceError> {
        let refresh_capability = query_handler(refresh_reference, &state.query.refresh_capability)
            .await
            .map_err(|err| RefreshCapabilityServiceError::Query(err.to_string()))?
            .ok_or(RefreshCapabilityServiceError::NotFound)?;

        if refresh_capability.status != RefreshCapabilityStatus::Active {
            return Err(RefreshCapabilityServiceError::NotFound);
        }

        Ok(ResolveRefreshCapabilityResponse {
            refresh_reference: refresh_capability.refresh_reference,
            credential_id: refresh_capability.credential_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use agent_issuance::refresh_capability::command::RefreshCapabilityCommand;
    use agent_issuance::refresh_capability::service::RefreshCapabilityService;
    use agent_issuance::refresh_capability::service::RefreshCapabilityServiceError;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::{initialize, IssuanceState};
    use agent_secret_manager::service::Service;
    use agent_shared::config::RefreshServiceConfiguration;
    use agent_shared::handlers::{command_handler, query_handler};
    use agent_store::{in_memory::InMemory, issuance_state};
    use std::sync::Arc;

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();
        state
    }

    #[async_std::test]
    async fn create_for_credential_skips_when_refresh_service_is_absent() {
        let state = test_state().await;

        let response = RefreshCapabilityService::default()
            .create_for_credential(&state, "credential-id", None)
            .await
            .unwrap();

        assert_eq!(response, None);
    }

    #[async_std::test]
    async fn create_for_credential_creates_capability_when_refresh_service_is_present() {
        let state = test_state().await;

        let response = RefreshCapabilityService::default()
            .create_for_credential(
                &state,
                "credential-id",
                Some(&RefreshServiceConfiguration {
                    type_: "VerifiableCredentialRefreshService2021".to_string(),
                }),
            )
            .await
            .unwrap()
            .unwrap();

        let refresh_capability = query_handler(&response.refresh_reference, &state.query.refresh_capability)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(refresh_capability.refresh_reference, response.refresh_reference);
        assert_eq!(refresh_capability.credential_id, "credential-id");
    }

    #[async_std::test]
    async fn resolve_active_returns_active_refresh_capability() {
        let state = test_state().await;

        let created = RefreshCapabilityService::default()
            .create_for_credential(
                &state,
                "credential-id",
                Some(&RefreshServiceConfiguration {
                    type_: "VerifiableCredentialRefreshService2021".to_string(),
                }),
            )
            .await
            .unwrap()
            .unwrap();

        let resolved = RefreshCapabilityService::default()
            .resolve_active(&state, &created.refresh_reference)
            .await
            .unwrap();

        assert_eq!(resolved.refresh_reference, created.refresh_reference);
        assert_eq!(resolved.credential_id, "credential-id");
    }

    #[async_std::test]
    async fn resolve_active_returns_not_found_for_unknown_reference() {
        let state = test_state().await;

        let error = RefreshCapabilityService::default()
            .resolve_active(&state, "unknown-refresh-reference")
            .await
            .expect_err("unknown reference should not resolve");

        assert!(matches!(error, RefreshCapabilityServiceError::NotFound));
    }

    #[async_std::test]
    async fn resolve_active_returns_not_found_for_disabled_reference() {
        let state = test_state().await;

        let created = RefreshCapabilityService::default()
            .create_for_credential(
                &state,
                "credential-id",
                Some(&RefreshServiceConfiguration {
                    type_: "VerifiableCredentialRefreshService2021".to_string(),
                }),
            )
            .await
            .unwrap()
            .unwrap();

        command_handler(
            &created.refresh_reference,
            &state.command.refresh_capability,
            RefreshCapabilityCommand::DisableRefreshCapability,
        )
        .await
        .unwrap();

        let error = RefreshCapabilityService::default()
            .resolve_active(&state, &created.refresh_reference)
            .await
            .expect_err("disabled reference should not resolve");

        assert!(matches!(error, RefreshCapabilityServiceError::NotFound));
    }
}
