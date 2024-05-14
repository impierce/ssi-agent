use agent_shared::handlers::{command_handler, query_handler};
use agent_verification::{
    authorization_request::queries::AuthorizationRequestView, connection::command::ConnectionCommand,
    generic_oid4vc::GenericAuthorizationResponse, state::VerificationState,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};

#[axum_macros::debug_handler]
pub(crate) async fn redirect(
    State(verification_state): State<VerificationState>,
    Form(authorization_response): Form<GenericAuthorizationResponse>,
) -> Response {
    let authorization_request_id = if let Some(state) = authorization_response.state() {
        state.clone()
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    // Retrieve the authorization request.
    let authorization_request = match query_handler(
        &authorization_request_id,
        &verification_state.query.authorization_request,
    )
    .await
    {
        Ok(Some(AuthorizationRequestView {
            authorization_request: Some(authorization_request),
            ..
        })) => authorization_request,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let connection_id = authorization_request.client_id();

    let command = ConnectionCommand::VerifyAuthorizationResponse {
        authorization_request,
        authorization_response,
    };

    // Verify the authorization response.
    if command_handler(&connection_id, &verification_state.command.connection, command)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::OK.into_response()
}

#[cfg(test)]
pub mod tests {
    use std::{str::FromStr, sync::Arc};

    use super::*;
    use crate::{
        app,
        verification::{authorization_requests::tests::authorization_requests, relying_party::request::tests::request},
    };
    use agent_event_publisher_http::{EventPublisherHttp, TEST_EVENT_PUBLISHER_HTTP_CONFIG};
    use agent_secret_manager::{secret_manager, subject::Subject};
    use agent_shared::config;
    use agent_store::{in_memory, EventPublisher};
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::{
        authorization_request::{AuthorizationRequest, Object},
        client_metadata::ClientMetadataResource,
        scope::Scope,
        DidMethod, SubjectSyntaxType,
    };
    use oid4vc_manager::ProviderManager;
    use siopv2::{authorization_request::ClientMetadataParameters, siopv2::SIOPv2};
    use tower::Service;
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
                },
            })
            .nonce("nonce".to_string())
            .state(state)
            .build()
            .unwrap();

        let provider_manager = ProviderManager::new(
            Arc::new(Subject {
                secret_manager: secret_manager().await,
            }),
            "did:key",
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

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_redirect_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/ssi-events-subscriber"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let target_url = format!("{}/ssi-events-subscriber", &mock_server.uri());

        TEST_EVENT_PUBLISHER_HTTP_CONFIG.lock().unwrap().replace(
            serde_yaml::from_str(&format!(
                r#"
                    target_url: &target_url {target_url}

                    connection: {{
                        target_url: *target_url,
                        target_events: [
                            SIOPv2AuthorizationResponseVerified
                        ]
                    }}
                "#,
            ))
            .unwrap(),
        );

        let event_publishers = vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn EventPublisher>];

        let issuance_state = in_memory::issuance_state(Default::default()).await;
        let verification_state = in_memory::verification_state(
            test_verification_services(&config!("default_did_method").unwrap_or("did:key".to_string())),
            event_publishers,
        )
        .await;

        let mut app = app((issuance_state, verification_state));

        let form_url_encoded_authorization_request = authorization_requests(&mut app).await;

        // Extract the state from the form_url_encoded_authorization_request.
        let state = form_url_encoded_authorization_request
            .split("%2F")
            .last()
            .unwrap()
            .to_string();

        request(&mut app, state.clone()).await;
        redirect(&mut app, state).await;

        // Assert that the event was dispatched to the target URL.
        assert!(mock_server.received_requests().await.unwrap().len() == 1);
    }
}
