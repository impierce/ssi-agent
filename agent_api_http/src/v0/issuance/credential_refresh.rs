use std::sync::Arc;

use crate::error::type_url;
use agent_issuance::{
    refresh_capability::{
        continuation::{
            PrepareRefreshContinuationRequest, RefreshContinuation, RefreshContinuationService,
            RefreshContinuationServiceError,
        },
        preparation::{NoOpRefreshPreparationHook, RefreshPreparationError},
        service::RefreshCapabilityServiceError,
    },
    state::IssuanceState,
};
use axum::{
    extract::{Json, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRefreshRequest {
    pub refresh_token: String,
}

#[utoipa::path(
    post,
    path = "/refresh-credential",
    operation_id = "refresh_credential",
    tags = ["Issuance"],
    request_body = CredentialRefreshRequest,
    responses(
        (status = 200, description = "Credential refresh continuation prepared successfully", content_type = "application/x-www-form-urlencoded", body = String),
        (status = 403, description = "Credential refresh cannot proceed"),
        (status = 404, description = "Refresh reference not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_refresh(
    State(state): State<Arc<IssuanceState>>,
    Json(CredentialRefreshRequest { refresh_token }): Json<CredentialRefreshRequest>,
) -> Result<Response, ApiError> {
    let continuation = RefreshContinuationService::new(NoOpRefreshPreparationHook)
        .prepare(
            state.as_ref(),
            PrepareRefreshContinuationRequest {
                refresh_reference: refresh_token,
            },
        )
        .await
        .map_err(refresh_continuation_error)?;

    match continuation {
        RefreshContinuation::CredentialOffer {
            form_url_encoded_credential_offer,
        } => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-www-form-urlencoded")],
            form_url_encoded_credential_offer,
        )
            .into_response()),
    }
}

fn refresh_continuation_error(error: RefreshContinuationServiceError) -> ApiError {
    match error {
        RefreshContinuationServiceError::RefreshCapability(RefreshCapabilityServiceError::NotFound) => {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("Refresh reference not found")
                .type_url(type_url("issuance#refresh-reference-not-found"))
                .message("The refresh reference was not found.")
                .finish()
        }
        RefreshContinuationServiceError::Preparation(RefreshPreparationError::RefreshUnavailable) => {
            ApiError::builder(StatusCode::FORBIDDEN)
                .title("Credential refresh cannot proceed")
                .type_url(type_url("issuance#credential-refresh-unavailable"))
                .message("Credential refresh cannot proceed.")
                .finish()
        }
        RefreshContinuationServiceError::Preparation(RefreshPreparationError::PreparationFailed(message)) => {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Credential refresh preparation failed")
                .type_url(type_url("issuance#credential-refresh-preparation-failed"))
                .message(message)
                .finish()
        }
        error => ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
            .title("Credential refresh failed")
            .type_url(type_url("issuance#credential-refresh-failed"))
            .message(error.to_string())
            .finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{v0::issuance::router, API_VERSION};
    use agent_issuance::{
        refresh_capability::{command::RefreshCapabilityCommand, service::RefreshCapabilityService},
        services::IssuanceServices,
        state::{initialize, IssuanceState},
    };
    use agent_secret_manager::service::Service;
    use agent_shared::{config::RefreshServiceConfiguration, handlers::command_handler};
    use agent_store::{in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use serde_json::json;
    use tower::ServiceExt;

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();
        state
    }

    async fn post_credential_refresh(state: Arc<IssuanceState>, refresh_token: &str) -> StatusCode {
        let app = router(state);

        app.oneshot(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}/refresh-credential"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "refreshToken": refresh_token
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }

    #[tokio::test]
    async fn credential_refresh_returns_not_found_for_unknown_reference() {
        let state = test_state().await;

        let status = post_credential_refresh(state, "unknown-refresh-reference").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn credential_refresh_returns_forbidden_when_noop_hook_cannot_prepare() {
        let state = test_state().await;
        let refresh_capability = RefreshCapabilityService::default()
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

        let status = post_credential_refresh(state, &refresh_capability.refresh_reference).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn credential_refresh_returns_not_found_for_disabled_reference() {
        let state = test_state().await;
        let refresh_capability = RefreshCapabilityService::default()
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
            &refresh_capability.refresh_reference,
            &state.command.refresh_capability,
            RefreshCapabilityCommand::DisableRefreshCapability,
        )
        .await
        .unwrap();

        let status = post_credential_refresh(state, &refresh_capability.refresh_reference).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
