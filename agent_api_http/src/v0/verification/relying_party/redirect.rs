use crate::handlers::public_command_handler;
use agent_verification::{
    authorization_request::command::AuthorizationRequestCommand, generic_oid4vc::GenericAuthorizationResponse,
    state::VerificationState,
};
use anyhow::{self, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use oid4vc_core::utils::form_urlencoded::from_form_urlencoded_string;
use std::sync::Arc;

#[axum_macros::debug_handler]
pub(crate) async fn redirect(
    State(verification_state): State<Arc<VerificationState>>,
    body: String,
) -> Result<Response, ApiError> {
    let authorization_response: GenericAuthorizationResponse = from_form_urlencoded_string(&body).map_err(|e| {
        tracing::error!("Failed to deserialize form data: {:?}", e);
        ApiError::new(StatusCode::BAD_REQUEST)
    })?;

    let authorization_request_id = if let Some(state) = authorization_response.state() {
        state.clone()
    } else {
        // TODO: Return a standardized error response.
        return Err(ApiError::new(StatusCode::BAD_REQUEST));
    };

    let command = AuthorizationRequestCommand::VerifyAuthorizationResponse { authorization_response };

    // Verify the authorization response.
    public_command_handler(
        &authorization_request_id,
        &verification_state.command.authorization_request,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[cfg(test)]
pub mod tests {
    use std::{str::FromStr, sync::Arc};

    use super::*;
    use crate::v0::verification::{
        authorization_requests::tests::authorization_requests, relying_party::request::tests::request, router,
    };
    use agent_secret_manager::{service::Service, subject::Subject};
    use agent_shared::config::{set_config, Events};
    use agent_store::{in_memory::InMemory, verification_state};
    use agent_verification::services::VerificationServices;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use jsonwebtoken::Algorithm;
    use oid4vc_core::{
        authorization_request::{AuthorizationRequest, Object},
        client_metadata::ClientMetadataResource,
        scope::Scope,
        DidMethod, SubjectSyntaxType,
    };
    use oid4vc_manager::ProviderManager;
    use siopv2::{authorization_request::ClientMetadataParameters, siopv2::SIOPv2};
    use tower::Service as _;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    pub async fn redirect(app: &mut Router, state: String) {
        let authorization_request = AuthorizationRequest::<Object<SIOPv2>>::builder()
            .client_id("client_id".to_string())
            .scope(Scope::openid())
            .redirect_uri("https://example.com".parse::<url::Url>().unwrap())
            .response_mode("direct_post".to_string())
            .client_metadata(ClientMetadataResource::ClientMetadata {
                client_name: None,
                logo_uri: None,
                extension: ClientMetadataParameters {
                    subject_syntax_types_supported: vec![SubjectSyntaxType::Did(
                        DidMethod::from_str("did:key").unwrap(),
                    )],
                    id_token_signed_response_alg: None,
                },
                other: Default::default(),
            })
            .nonce("nonce".to_string())
            .state(state)
            .build()
            .unwrap();

        let provider_manager = ProviderManager::new(
            Arc::new(Subject::test_subject().await),
            vec!["did:key"],
            vec![Algorithm::EdDSA],
        )
        .unwrap();
        let authorization_response = provider_manager
            .generate_response(&authorization_request, Default::default())
            .await
            .unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/redirect")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(serde_urlencoded::to_string(authorization_response).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[serial_test::serial]
    #[tokio::test(flavor = "multi_thread")]
    #[tracing_test::traced_test]
    async fn test_redirect_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ssi-events-subscriber"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let target_url = format!("{}/ssi-events-subscriber", &mock_server.uri());

        set_config().enable_event_publisher_http(0);
        set_config().set_event_publisher_http_target_url(0, target_url.clone());
        set_config().set_event_publisher_http_target_events(
            0,
            Events {
                authorization_request: vec![
                    agent_shared::config::AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified,
                ],
                ..Default::default()
            },
        );

        let bus = shared_kernel::event_bus::EventBusHandle::new(1024);
        agent_event_publisher_http::start_http_forwarder(bus.clone());

        let verification_state =
            Arc::new(verification_state(&InMemory, VerificationServices::default().await, bus).await);

        let mut app = router(verification_state);

        let form_url_encoded_authorization_request = authorization_requests(&mut app).await;

        // Extract the state from the form_url_encoded_authorization_request.
        let state = form_url_encoded_authorization_request
            .split("%2F")
            .last()
            .unwrap()
            .to_string();

        request(&mut app, state.clone()).await;
        redirect(&mut app, state).await;

        // Wait for the request to arrive at the mock server endpoint.
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Assert that the event was dispatched to the target URL.
        assert!(mock_server.received_requests().await.unwrap().len() == 1);
    }
}
