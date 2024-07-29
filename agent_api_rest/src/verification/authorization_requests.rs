use crate::API_VERSION;
use agent_shared::{
    generate_random_string,
    handlers::{command_handler, query_handler},
};
use agent_verification::{
    authorization_request::{command::AuthorizationRequestCommand, queries::AuthorizationRequestView},
    state::VerificationState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hyper::header;
use oid4vp::PresentationDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn get_authorization_requests(
    State(state): State<VerificationState>,
    Path(authorization_request_id): Path<String>,
) -> Response {
    // Get the authorization request if it exists.
    match query_handler(&authorization_request_id, &state.query.authorization_request).await {
        Ok(Some(AuthorizationRequestView {
            authorization_request: Some(authorization_request),
            ..
        })) => (StatusCode::OK, Json(authorization_request)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
pub struct AuthorizationRequestsEndpointRequest {
    pub nonce: String,
    pub state: Option<String>,
    #[serde(flatten)]
    pub presentation_definition: Option<PresentationDefinitionResource>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationDefinitionResource {
    PresentationDefinitionId(String),
    PresentationDefinition(PresentationDefinition),
}

#[axum_macros::debug_handler]
pub(crate) async fn authorization_requests(
    State(verification_state): State<VerificationState>,
    Json(payload): Json<Value>,
) -> Response {
    info!("Request Body: {}", payload);

    let Ok(AuthorizationRequestsEndpointRequest {
        nonce,
        state,
        presentation_definition,
    }) = serde_json::from_value(payload)
    else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    let state = state.unwrap_or(generate_random_string());

    let presentation_definition = presentation_definition.map(|presentation_definition| {
        match presentation_definition {
            // TODO: This needs to be properly fixed instead of reading the presentation definitions from the file system
            // everytime a request is made. `PresentationDefinition`'s should be implemented as a proper `Aggregate`. This
            // current suboptimal solution requires the `./tmp:/app/agent_api_rest` volume to be mounted in the `docker-compose.yml`.
            PresentationDefinitionResource::PresentationDefinitionId(presentation_definition_id) => {
                let project_root_dir = env!("CARGO_MANIFEST_DIR");

                serde_json::from_reader(
                    std::fs::File::open(format!(
                        "{project_root_dir}/../agent_verification/presentation_definitions/{presentation_definition_id}.json"
                    ))
                    .unwrap(),
                )
                .unwrap()
            }
            PresentationDefinitionResource::PresentationDefinition(presentation_definition) => presentation_definition,
        }
    });

    let command = AuthorizationRequestCommand::CreateAuthorizationRequest {
        nonce: nonce.to_string(),
        state: state.clone(),
        presentation_definition,
    };

    // Create the authorization request.
    if command_handler(&state, &verification_state.command.authorization_request, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Sign the authorization request object.
    if command_handler(
        &state,
        &verification_state.command.authorization_request,
        AuthorizationRequestCommand::SignAuthorizationRequestObject,
    )
    .await
    .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Return the credential.
    match query_handler(&state, &verification_state.query.authorization_request).await {
        Ok(Some(AuthorizationRequestView {
            form_url_encoded_authorization_request: Some(form_url_encoded_authorization_request),
            ..
        })) => (
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
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::app;
    use agent_store::in_memory;
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use rstest::rstest;
    use tower::Service;

    pub async fn authorization_requests(app: &mut Router, by_value: bool) -> String {
        let request_body = AuthorizationRequestsEndpointRequest {
            nonce: "nonce".to_string(),
            state: None,
            presentation_definition: Some(if by_value {
                PresentationDefinitionResource::PresentationDefinition(
                    serde_json::from_str(include_str!(
                        "../../../agent_verification/presentation_definitions/presentation_definition.json"
                    ))
                    .unwrap(),
                )
            } else {
                PresentationDefinitionResource::PresentationDefinitionId("presentation_definition".to_string())
            }),
        };

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(&format!("{API_VERSION}/authorization_requests"))
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

        let state = get_request_endpoint.split('/').last().unwrap().to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let form_url_encoded_authorization_request: String = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(form_url_encoded_authorization_request, format!("openid://?client_id=did%3Akey%3Az6MkgE84NCMpMeAx9jK9cf5W4G8gcZ9xuwJvG1e7wNk8KCgt&request_uri=https%3A%2F%2Fmy-domain.example.org%2Frequest%2F{state}"));

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

    #[rstest]
    #[case::with_presentation_definition_by_value(true)]
    #[case::with_presentation_definition_id(false)]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_authorization_requests_endpoint(#[case] by_value: bool) {
        let issuance_state = in_memory::issuance_state(Default::default()).await;
        let verification_state = in_memory::verification_state(test_verification_services(), Default::default()).await;
        let mut app = app((issuance_state, verification_state));

        authorization_requests(&mut app, by_value).await;
    }
}
