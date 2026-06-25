use crate::error::{type_url, IntoApiErrorExt};
use crate::handlers::query_handler;
use agent_issuance::{
    credential::aggregate::CredentialExpiry,
    reissuance::{
        service::{CreateReissuanceRequest, ReissuanceService},
        views::ReissuanceView,
    },
    state::IssuanceState,
};
use axum::{
    extract::{Json, Path, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialReissuanceRequest {
    pub original_credential_id: String,
    pub credential_configuration_id: String,
    pub credential: serde_json::Value,
    pub expires_at: CredentialExpiry,
    pub reason: Option<String>,
    // TODO stronger types
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
    pub status_action: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialReissuanceResponse {
    pub id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,
    pub credential_offer: Option<String>,
}

#[utoipa::path(
    post,
    path = "/reissue-credential",
    operation_id = "reissue_credential",
    tags = ["Issuance"],
    responses(
        (status = 201, description = "Credential reissuance prepared successfully", body = CreateCredentialReissuanceResponse)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_reissuances(
    State(state): State<Arc<IssuanceState>>,
    Json(CreateCredentialReissuanceRequest {
        original_credential_id,
        credential_configuration_id,
        credential,
        expires_at,
        reason,
        trigger_type,
        triggered_by,
        status_action,
    }): Json<CreateCredentialReissuanceRequest>,
) -> Result<Response, ApiError> {
    let service = ReissuanceService::default();

    let request = CreateReissuanceRequest {
        reissuance_id: uuid::Uuid::new_v4().to_string(),
        original_credential_id,
        new_credential_id: uuid::Uuid::new_v4().to_string(),
        offer_id: uuid::Uuid::new_v4().to_string(),
        credential_configuration_id,
        credential,
        expires_at,
        reason,
        trigger_type,
        triggered_by,
        status_action,
    };

    let response = service
        .create(&state, request)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    let credential_offer = query_handler(&response.offer_id, &state.query.offer)
        .await
        .map_err(|_| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Failed to query credential offer")
                .type_url(type_url("issuance#query-credential-offer-failed"))
                .message("Failed to query the prepared credential offer.")
                .finish()
        })?
        .and_then(|offer| offer.form_url_encoded_credential_offer);

    Ok((
        StatusCode::CREATED,
        Json(CreateCredentialReissuanceResponse {
            id: response.reissuance_id,
            original_credential_id: response.original_credential_id,
            new_credential_id: response.new_credential_id,
            offer_id: response.offer_id,
            credential_configuration_id: response.credential_configuration_id,
            credential_offer,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/list-all-credential-reissuances",
    operation_id = "list_all_credential_reissuances",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "All credential reissuance relations retrieved successfully", body = [ReissuanceView])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_credential_reissuances(State(state): State<Arc<IssuanceState>>) -> Result<Response, ApiError> {
    let reissuances = query_handler("all_reissuances", &state.query.all_reissuances)
        .await
        .map_err(|_| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Failed to query credential reissuances")
                .type_url(type_url("issuance#query-credential-reissuances-failed"))
                .message("Failed to query the credential reissuance relations.")
                .finish()
        })?
        .map(|all_reissuances_view| all_reissuances_view.reissuances.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(reissuances)).into_response())
}

#[utoipa::path(
    get,
    path = "/get-credential-reissuance/{id}",
    operation_id = "get_credential_reissuance",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Credential reissuance relation retrieved successfully", body = ReissuanceView),
        (status = 404, description = "Credential reissuance relation not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_reissuance(
    State(state): State<Arc<IssuanceState>>,
    Path(reissuance_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&reissuance_id, &state.query.reissuance)
        .await
        .map_err(|_| {
            ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                .title("Failed to query credential reissuance")
                .type_url(type_url("issuance#query-credential-reissuance-failed"))
                .message("Failed to query the credential reissuance relation.")
                .finish()
        })?
        .map(|reissuance_view| (StatusCode::OK, Json(reissuance_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        v0::{
            authorization,
            authorization::authorization_server::token::tests::token,
            issuance::{credential_issuer::credential::tests::TEST_NONCE, router},
        },
        API_VERSION,
    };
    use agent_authorization::services::AuthorizationServices;
    use agent_issuance::{
        credential::{aggregate::Status as CredentialStatus, command::CredentialCommand, entity::Data},
        nonce::command::NonceCommand,
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, SERVER_CONFIG_ID},
    };
    use agent_secret_manager::service::Service;
    use agent_shared::{
        config::CredentialConfiguration,
        handlers::{command_handler, query_handler},
    };
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::{self, Body},
        http::{self, header, Method, Request},
    };
    use oid4vci::credential_offer::AuthorizationCode;
    use serde_json::{json, Value};
    use serial_test::serial;
    use tower::ServiceExt;

    const CREDENTIAL_PROOF_JWT: &str = "eyJ0eXAiOiJvcGVuaWQ0dmNpLXByb29mK2p3dCIsImFsZyI6IkVkRFNBIiwia2lkIjoiZGlkOmtleTp6Nk1raWlleW9MTVNWc0pBWnY3SmplNXdXU2tERXltVWdreUY4a2JjcmpacFgzcWQjejZNa2lpZXlvTE1TVnNKQVp2N0pqZTV3V1NrREV5bVVna3lGOGtiY3JqWnBYM3FkIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtpaWV5b0xNU1ZzSkFadjdKamU1d1dTa0RFeW1VZ2t5RjhrYmNyalpwWDNxZCIsImF1ZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjE1NzEzMjQ4MDAsIm5vbmNlIjoiN2UwM2FkM2Y3NmNiMzMzOGMzYTU2NDJmZTc2MzQ0NzZhYTNhZDkzZmExZDU4NDAxMWJhMjE1MGQ5ZGE0NzEzMyJ9.bDxmEWTGwKJJC8J5N16JHAR2ZBYtgWlhM_o_voJdXLnw_ScZMwGjZwNH6aQWKlgIaFWKonF88KNRFX2UAOAuBQ";

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();
        add_sd_jwt_credential_configuration(&state).await;
        state
    }

    async fn add_sd_jwt_credential_configuration(state: &IssuanceState) {
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
                    "path": ["first_name"],
                    "display": [{ "name": "First Name", "locale": "en" }]
                },
                {
                    "path": ["last_name"],
                    "display": [{ "name": "Last Name", "locale": "en" }]
                },
                {
                    "path": ["dob"],
                    "display": [{ "name": "Date of Birth", "locale": "en" }]
                }
            ]
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
        let credential_configuration = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations
            .get("SD-JWT VC")
            .unwrap()
            .1
            .clone();

        command_handler(
            "original-credential-id",
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: "original-credential-id".to_string(),
                data: Data {
                    raw: json!({
                        "first_name": "Ferris",
                        "last_name": "Rustacean",
                        "dob": "2010-01-01"
                    }),
                },
                credential_configuration: Box::new(credential_configuration),
                refresh_service: None,
                expires_at: CredentialExpiry::Never,
            },
        )
        .await
        .unwrap();
    }

    async fn post_reissuance(state: Arc<IssuanceState>) -> (StatusCode, Value) {
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/reissue-credential"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "originalCredentialId": "original-credential-id",
                            "credentialConfigurationId": "SD-JWT VC",
                            "credential": {
                                "first_name": "Ferris",
                                "last_name": "Reissued",
                                "dob": "2010-01-01"
                            },
                            "expiresAt": "never",
                            "reason": "data_changed",
                            "triggerType": "manual",
                            "triggeredBy": "unitrust"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn test_credential_reissuance_endpoint_prepares_offer_and_relation() {
        let state = test_state().await;
        create_original_credential(&state).await;

        let (status, body) = post_reissuance(state.clone()).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["originalCredentialId"], "original-credential-id");
        assert_eq!(body["credentialConfigurationId"], "SD-JWT VC");
        assert!(body["id"].is_string());
        assert!(body["newCredentialId"].is_string());
        assert!(body["offerId"].is_string());
        assert!(body["credentialOffer"].is_string());

        let reissuance_id = body["id"].as_str().unwrap();
        let new_credential_id = body["newCredentialId"].as_str().unwrap();
        let offer_id = body["offerId"].as_str().unwrap();

        let reissuance = query_handler(reissuance_id, &state.query.reissuance)
            .await
            .unwrap()
            .unwrap();
        let new_credential = query_handler(new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();
        let offer = query_handler(offer_id, &state.query.offer).await.unwrap().unwrap();

        assert_eq!(reissuance.original_credential_id, "original-credential-id");
        assert_eq!(reissuance.new_credential_id, new_credential_id);
        assert_eq!(reissuance.offer_id, offer_id);
        assert_eq!(reissuance.status_action, None);
        assert_eq!(new_credential.data.unwrap().raw["last_name"], json!("Reissued"));
        assert_eq!(offer.credential_ids, vec![new_credential_id.to_string()]);
    }

    #[tokio::test]
    #[serial]
    async fn test_reissued_credential_can_be_issued_through_oid4vci() {
        let state = test_state().await;
        create_original_credential(&state).await;

        let (status, body) = post_reissuance(state.clone()).await;

        assert_eq!(status, StatusCode::CREATED);
        let offer_id = body["offerId"].as_str().unwrap();
        let new_credential_id = body["newCredentialId"].as_str().unwrap();

        let authorization_state =
            Arc::new(authorization_state(&InMemory, AuthorizationServices::default().await, Default::default()).await);
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut authorization_app = authorization::router((authorization_state, state.clone()));
        let access_token = token(
            &mut authorization_app,
            false,
            (
                Some(AuthorizationCode {
                    issuer_state: Some(offer_id.to_string()),
                    authorization_server: None,
                }),
                None,
            ),
        )
        .await;

        command_handler(
            TEST_NONCE,
            &state.command.nonce,
            NonceCommand::GenerateNonce {
                c_nonce: TEST_NONCE.to_string(),
            },
        )
        .await
        .unwrap();

        let app = router(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/openid4vci/credential")
                    .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "credential_configuration_id": "SD-JWT VC",
                            "proofs": {
                                "jwt": [CREDENTIAL_PROOF_JWT]
                            }
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );

        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert!(body["credentials"][0]["credential"].is_string());

        let credential = query_handler(new_credential_id, &state.query.credential)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(credential.status, CredentialStatus::Issued);
        assert!(credential.signed.is_some());
    }

    #[tokio::test]
    async fn test_all_credential_reissuances_endpoint_returns_relations() {
        let state = test_state().await;
        create_original_credential(&state).await;
        let (status, created_body) = post_reissuance(state.clone()).await;
        assert_eq!(status, StatusCode::CREATED);

        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/list-all-credential-reissuances"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body.as_array().unwrap().len(), 1);
        assert_eq!(body[0]["id"], created_body["id"]);
        assert_eq!(body[0]["original_credential_id"], "original-credential-id");
    }

    #[tokio::test]
    async fn test_credential_reissuance_endpoint_returns_relation_by_id() {
        let state = test_state().await;
        create_original_credential(&state).await;
        let (status, created_body) = post_reissuance(state.clone()).await;
        assert_eq!(status, StatusCode::CREATED);

        let reissuance_id = created_body["id"].as_str().unwrap();
        let app = router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/get-credential-reissuance/{reissuance_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["id"], created_body["id"]);
        assert_eq!(body["original_credential_id"], "original-credential-id");
        assert_eq!(body["new_credential_id"], created_body["newCredentialId"]);
    }

    #[tokio::test]
    async fn test_credential_reissuance_endpoint_returns_not_found_for_unknown_id() {
        let state = test_state().await;
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!("{API_VERSION}/get-credential-reissuance/unknown-reissuance-id"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
