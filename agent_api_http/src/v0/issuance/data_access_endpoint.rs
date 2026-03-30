use std::sync::Arc;

use agent_issuance::application::data_access_service::DataAccessService;
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
pub struct DataAccessRequest {
    #[serde(rename = "data-access-consent-token")]
    data_access_consent_token: String,
}

/// This endpoint receives a Data Access Consent Token as a query parameter and then perform several validation steps on the token.
/// When all validations pass, the requested credential is returned in the response.
/// When any validation fails, only the error is returned.
/// Both the verifier and the Issuer need to perform all these checks on the Data Access Consent Token, zero trust is assumed.
pub async fn data_access_endpoint(
    State(state): State<Arc<IssuanceState>>,
    Json(payload): Json<DataAccessRequest>,
) -> Result<Response, ApiError> {
    let public_credential_service = DataAccessService {};

    let requested_credential = public_credential_service
        .resolve_data_access_consent_token(payload.data_access_consent_token, &state)
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
    async fn test_data_access_endpoint_invalid_body() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/data-access-endpoint"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    }
}
