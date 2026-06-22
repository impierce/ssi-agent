use agent_shared::handlers::query_handler;

use crate::{
    refresh_capability::{
        preparation::{RefreshPreparationError, RefreshPreparationHook, RefreshPreparationRequest},
        service::{RefreshCapabilityService, RefreshCapabilityServiceError},
    },
    reissuance::service::{CreateReissuanceRequest, ReissuanceService, ReissuanceServiceError},
    state::IssuanceState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PrepareRefreshContinuationRequest {
    pub refresh_reference: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RefreshContinuation {
    CredentialOffer { form_url_encoded_credential_offer: String },
}

#[derive(Debug, thiserror::Error)]
pub enum RefreshContinuationServiceError {
    #[error(transparent)]
    RefreshCapability(#[from] RefreshCapabilityServiceError),
    #[error(transparent)]
    Preparation(#[from] RefreshPreparationError),
    #[error(transparent)]
    Reissuance(#[from] ReissuanceServiceError),
    #[error("Credential offer was not found")]
    CredentialOfferNotFound,
    #[error("Failed to query state: {0}")]
    Query(String),
}

pub struct RefreshContinuationService<H> {
    preparation_hook: H,
}

impl<H> RefreshContinuationService<H>
where
    H: RefreshPreparationHook,
{
    pub fn new(preparation_hook: H) -> Self {
        Self { preparation_hook }
    }

    pub async fn prepare(
        &self,
        state: &IssuanceState,
        request: PrepareRefreshContinuationRequest,
    ) -> Result<RefreshContinuation, RefreshContinuationServiceError> {
        let resolved = RefreshCapabilityService::default()
            .resolve_active(state, &request.refresh_reference)
            .await?;

        let input = self
            .preparation_hook
            .prepare(&RefreshPreparationRequest {
                refresh_reference: resolved.refresh_reference.clone(),
                credential_id: resolved.credential_id.clone(),
            })
            .await?;

        let response = ReissuanceService::default()
            .create(
                state,
                CreateReissuanceRequest {
                    reissuance_id: uuid::Uuid::new_v4().to_string(),
                    original_credential_id: resolved.credential_id,
                    new_credential_id: uuid::Uuid::new_v4().to_string(),
                    offer_id: uuid::Uuid::new_v4().to_string(),
                    credential_configuration_id: input.credential_configuration_id,
                    credential: input.credential,
                    expires_at: input.expires_at,
                    reason: input.metadata.reason,
                    trigger_type: input.metadata.trigger_type,
                    triggered_by: input.metadata.triggered_by,
                    status_action: None,
                },
            )
            .await?;

        let form_url_encoded_credential_offer = query_handler(&response.offer_id, &state.query.offer)
            .await
            .map_err(|err| RefreshContinuationServiceError::Query(err.to_string()))?
            .and_then(|offer| offer.form_url_encoded_credential_offer)
            .ok_or(RefreshContinuationServiceError::CredentialOfferNotFound)?;

        Ok(RefreshContinuation::CredentialOffer {
            form_url_encoded_credential_offer,
        })
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use agent_issuance::{
        credential::{aggregate::CredentialExpiry, command::CredentialCommand, entity::Data},
        refresh_capability::{
            continuation::{
                PrepareRefreshContinuationRequest, RefreshContinuation, RefreshContinuationService,
                RefreshContinuationServiceError,
            },
            preparation::{
                RefreshPreparationError, RefreshPreparationHook, RefreshPreparationInput, RefreshPreparationMetadata,
                RefreshPreparationRequest,
            },
            service::RefreshCapabilityService,
        },
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, IssuanceState, SERVER_CONFIG_ID},
    };
    use agent_secret_manager::service::Service;
    use agent_shared::{
        config::CredentialConfiguration,
        handlers::{command_handler, query_handler},
        UrlAppendHelpers,
    };
    use agent_store::{in_memory::InMemory, issuance_state};
    use serde_json::json;
    use std::sync::Arc;

    struct TestRefreshPreparationHook;

    #[async_trait]
    impl RefreshPreparationHook for TestRefreshPreparationHook {
        async fn prepare(
            &self,
            request: &RefreshPreparationRequest,
        ) -> Result<RefreshPreparationInput, RefreshPreparationError> {
            Ok(RefreshPreparationInput {
                credential_configuration_id: "SD-JWT VC".to_string(),
                credential: json!({
                    "first_name": "Ferris",
                    "last_name": "Refreshed",
                    "dob": "2010-01-01"
                }),
                expires_at: CredentialExpiry::Never,
                metadata: RefreshPreparationMetadata {
                    reason: Some("test".to_string()),
                    trigger_type: Some("refresh_service".to_string()),
                    triggered_by: Some(request.refresh_reference.clone()),
                },
            })
        }
    }

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(state.as_ref()).await.unwrap();
        state
    }

    async fn make_sd_jwt_configuration_refreshable(state: &IssuanceState) {
        let credential_configuration = serde_json::from_value::<CredentialConfiguration>(json!({
            "credential_configuration_id": "SD-JWT VC",
            "format": "dc+sd-jwt",
            "display": [
                {
                    "name": "SD-JWT VC Credential",
                    "locale": "en"
                }
            ],
            "claims": [
                {
                    "path": ["first_name"]
                },
                {
                    "path": ["last_name"]
                },
                {
                    "path": ["dob"]
                }
            ],
            "refreshService": {
                "type": "VerifiableCredentialRefreshService2021"
            }
        }))
        .unwrap();

        command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration,
                provisioned: false,
            },
        )
        .await
        .unwrap();
    }

    async fn create_original_credential(state: &IssuanceState) {
        let (_, credential_configuration, _, refresh_service) =
            query_handler(SERVER_CONFIG_ID, &state.query.server_config)
                .await
                .unwrap()
                .and_then(|server_config| server_config.credential_configurations.get("SD-JWT VC").cloned())
                .unwrap();

        let refresh_capability = RefreshCapabilityService::default()
            .create_for_credential(state, "original-credential-id", refresh_service.as_ref())
            .await
            .unwrap();

        let credential_refresh_service =
            refresh_service
                .as_ref()
                .zip(refresh_capability.as_ref())
                .map(|(refresh_service, refresh_capability)| {
                    agent_issuance::credential::aggregate::CredentialRefreshService {
                        type_: refresh_service.type_.clone(),
                        url: agent_shared::config::config()
                            .public_url
                            .append_path_segment("credential-refresh")
                            .to_string(),
                        refresh_token: refresh_capability.refresh_reference.clone(),
                    }
                });

        command_handler(
            "original-credential-id",
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: "original-credential-id".to_string(),
                data: Data {
                    raw: json!({
                        "first_name": "Ferris",
                        "last_name": "Original",
                        "dob": "2010-01-01"
                    }),
                },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
                refresh_service: credential_refresh_service,
            },
        )
        .await
        .unwrap();
    }

    #[async_std::test]
    async fn prepare_returns_credential_offer_continuation() {
        let state = test_state().await;
        make_sd_jwt_configuration_refreshable(&state).await;
        create_original_credential(&state).await;

        let original_credential = query_handler("original-credential-id", &state.query.credential)
            .await
            .unwrap()
            .unwrap();

        let refresh_reference = original_credential
            .refresh_service
            .expect("original credential should be refreshable")
            .refresh_token;

        let continuation = RefreshContinuationService::new(TestRefreshPreparationHook)
            .prepare(state.as_ref(), PrepareRefreshContinuationRequest { refresh_reference })
            .await
            .unwrap();

        let RefreshContinuation::CredentialOffer {
            form_url_encoded_credential_offer,
        } = continuation;

        assert!(form_url_encoded_credential_offer.starts_with("openid-credential-offer://"));

        let all_reissuances = query_handler("all_reissuances", &state.query.all_reissuances)
            .await
            .unwrap()
            .unwrap();

        let reissuance = all_reissuances
            .reissuances
            .values()
            .find(|reissuance| reissuance.original_credential_id == "original-credential-id")
            .expect("refresh should create a reissuance relation");

        assert_eq!(reissuance.reason.as_deref(), Some("test"));
        assert_eq!(reissuance.trigger_type.as_deref(), Some("refresh_service"));
    }

    struct DenyingRefreshPreparationHook;

    #[async_trait]
    impl RefreshPreparationHook for DenyingRefreshPreparationHook {
        async fn prepare(
            &self,
            _request: &RefreshPreparationRequest,
        ) -> Result<RefreshPreparationInput, RefreshPreparationError> {
            Err(RefreshPreparationError::RefreshUnavailable)
        }
    }

    #[async_std::test]
    async fn denial_should_fail_refresh_continuation() {
        let state = test_state().await;
        make_sd_jwt_configuration_refreshable(&state).await;
        create_original_credential(&state).await;

        let original_credential = query_handler("original-credential-id", &state.query.credential)
            .await
            .unwrap()
            .unwrap();

        let refresh_reference = original_credential
            .refresh_service
            .expect("original credential should be refreshable")
            .refresh_token;

        let error = RefreshContinuationService::new(DenyingRefreshPreparationHook)
            .prepare(&state, PrepareRefreshContinuationRequest { refresh_reference })
            .await
            .expect_err("hook denial should fail refresh continuation");

        assert!(matches!(
            error,
            RefreshContinuationServiceError::Preparation(RefreshPreparationError::RefreshUnavailable)
        ));
    }
}
