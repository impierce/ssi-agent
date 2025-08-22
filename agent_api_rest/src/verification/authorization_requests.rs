use crate::{
    handlers::{command_handler, query_handler},
    API_VERSION,
};
use agent_shared::generate_random_string;
use agent_verification::{authorization_request::command::AuthorizationRequestCommand, state::VerificationState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::header;
use oid4vp::dcql::dcql_query::DcqlQuery;
use serde::{Deserialize, Serialize};

#[axum_macros::debug_handler]
pub(crate) async fn all_authorization_requests(State(state): State<VerificationState>) -> Result<Response, ApiError> {
    let all_authorization_requests =
        query_handler("all_authorization_requests", &state.query.all_authorization_requests)
            .await?
            .map(|all_authorization_requests_view| {
                all_authorization_requests_view
                    .authorization_requests
                    .into_values()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_authorization_requests)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn authorization_request(
    State(state): State<VerificationState>,
    Path(authorization_request_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&authorization_request_id, &state.query.authorization_request)
        .await?
        .map(|authorization_request_view| (StatusCode::OK, Json(authorization_request_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize)]
pub struct AuthorizationRequestsEndpointRequest {
    pub nonce: String,
    pub state: Option<String>,
    pub dcql_query: Option<DcqlQuery>,
}

#[axum_macros::debug_handler]
pub(crate) async fn authorization_requests(
    State(verification_state): State<VerificationState>,
    Json(AuthorizationRequestsEndpointRequest {
        nonce,
        state,
        dcql_query,
    }): Json<AuthorizationRequestsEndpointRequest>,
) -> Result<Response, ApiError> {
    let state = state.unwrap_or(generate_random_string());

    let command = AuthorizationRequestCommand::CreateAuthorizationRequest {
        nonce: nonce.to_string(),
        state: state.clone(),
        dcql_query: dcql_query.clone(),
    };

    // Create the authorization request.
    command_handler(&state, &verification_state.command.authorization_request, command).await?;

    // Sign the authorization request object.
    command_handler(
        &state,
        &verification_state.command.authorization_request,
        AuthorizationRequestCommand::SignAuthorizationRequestObject,
    )
    .await?;

    // Return the authorization_request.
    query_handler(&state, &verification_state.query.authorization_request)
        .await?
        .and_then(|authorization_request_view| authorization_request_view.form_url_encoded_authorization_request)
        .map(|form_url_encoded_authorization_request| {
            (
                StatusCode::CREATED,
                [
                    (
                        header::LOCATION,
                        format!("{API_VERSION}/authorization_requests/{state}").as_str(),
                    ),
                    (header::CONTENT_TYPE, "application/x-www-form-urlencoded"),
                ],
                form_url_encoded_authorization_request,
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::verification::router;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::verification_state;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use lazy_static::lazy_static;
    use serde_json::json;
    use tower::Service as _;

    lazy_static! {
        pub static ref DCQL_QUERY: DcqlQuery = serde_json::from_value(json!({
            "credentials": [
                {
                    "id": "my_credential",
                    "format": "jwt_vc_json",
                    "meta": {
                        "vct_values": [ "https://www.w3.org/2018/credentials/examples/v1#PersonalInformation" ]
                    },
                    "claims": [
                        {"path": ["credentialSubject", "familyName"]},
                        {"path": ["credentialSubject", "givenName"]},
                        {"path": ["credentialSubject", "email"]},
                        {"path": ["credentialSubject", "birthdate"]},
                    ]
                }
            ]
        }))
        .unwrap();
    }

    pub async fn authorization_requests(app: &mut Router) -> String {
        let request_body = AuthorizationRequestsEndpointRequest {
            nonce: "nonce".to_string(),
            state: None,
            dcql_query: Some(DCQL_QUERY.clone()),
        };

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/authorization_requests"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/x-www-form-urlencoded"
        );

        let get_request_endpoint = response
            .headers()
            .get(http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let state = get_request_endpoint.split('/').next_back().unwrap().to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let form_url_encoded_authorization_request: String = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(form_url_encoded_authorization_request, format!("openid://?client_id=decentralized_identifier%3Adid%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2F{state}"));
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri(get_request_endpoint)
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        form_url_encoded_authorization_request
    }

    #[tracing_test::traced_test]
    #[tokio::test]
    async fn test_authorization_requests_endpoint() {
        let verification_state = verification_state::<InMemory>(Service::default(), Default::default()).await;
        let mut app = router(verification_state);

        let result = authorization_requests(&mut app).await;
        assert!(!result.is_empty());
    }
}
