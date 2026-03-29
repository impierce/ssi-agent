use agent_issuance::application::public_credential_service::PublicCredentialService;
use agent_issuance::state::IssuanceState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use serde::Deserialize;

use crate::error::IntoApiErrorExt;

#[derive(Deserialize)]
pub struct PublicLinkQuery {
    #[serde(rename = "public-credential-token")]
    data_access_consent_token: String,
}

/// This endpoint receives a Public Credential Token as a query parameter and then perform several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response.
/// When any validation fails, only the error is returned.
/// Both the verifier and the Issuer need to perform all these checks on the Public Credential Token, zero trust is assumed.
pub async fn public_credential(
    State(state): State<IssuanceState>,
    Json(payload): Json<PublicLinkQuery>,
) -> Result<Response, ApiError> {
    let public_credential_service = PublicCredentialService {};

    let requested_credential = public_credential_service
        .get_public_credential(payload.data_access_consent_token, &state)
        .await
        .map_err(|e| e.into_api_error())?;

    Ok((StatusCode::OK, Json(requested_credential)).into_response())
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use crate::v0::issuance::router;
    use crate::API_VERSION;
    use agent_issuance::{services::IssuanceServices, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use tower::Service as _;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_public_credential_endpoint_invalid_parameter() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(format!(
                        "{API_VERSION}/public-credential?public_credential_token=invalid"
                    ))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    }
}
