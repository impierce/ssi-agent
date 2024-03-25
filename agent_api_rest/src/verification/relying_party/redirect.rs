use agent_shared::handlers::{command_handler, query_handler};
use agent_verification::{
    authorization_request::queries::AuthorizationRequestView, connection::command::ConnectionCommand,
    state::VerificationState,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Form,
};
use oid4vc_core::authorization_response::AuthorizationResponse;
use siopv2::siopv2::SIOPv2;

#[axum_macros::debug_handler]
pub(crate) async fn redirect(
    State(verification_state): State<VerificationState>,
    Form(siopv2_authorization_response): Form<AuthorizationResponse<SIOPv2>>,
) -> Response {
    let authorization_request_id = if let Some(state) = siopv2_authorization_response.state.as_ref() {
        state.clone()
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let siopv2_authorization_request = match query_handler(
        &authorization_request_id,
        &verification_state.query.authorization_request,
    )
    .await
    {
        Ok(Some(AuthorizationRequestView {
            siopv2_authorization_request: Some(siopv2_authorization_request),
            ..
        })) => siopv2_authorization_request,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let connection_id = if let Some(state) = siopv2_authorization_request.body.state.as_ref() {
        state.clone()
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };

    let command = ConnectionCommand::VerifySIOPv2AuthorizationResponse {
        siopv2_authorization_request,
        siopv2_authorization_response,
    };

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
    use agent_shared::secret_manager::secret_manager;
    use agent_store::{in_memory, OutboundAdapter};
    use agent_verification::services::test_utils::test_verification_services;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::{
        authorization_request::{AuthorizationRequest, Object},
        client_metadata::ClientMetadata,
        scope::Scope,
        DidMethod, SubjectSyntaxType,
    };
    use oid4vc_manager::ProviderManager;
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
            .client_metadata(ClientMetadata::default().with_subject_syntax_types_supported(vec![
                SubjectSyntaxType::Did(DidMethod::from_str("did:test").unwrap()),
            ]))
            .nonce("nonce".to_string())
            .state(state)
            .build()
            .unwrap();

        let provider_manager =
            ProviderManager::new([Arc::new(futures::executor::block_on(async { secret_manager().await }))]).unwrap();
        let authorization_response = provider_manager
            .generate_response(&authorization_request, Default::default())
            .unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/siopv2/redirect")
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

        let outbound_adapters = vec![Box::new(EventPublisherHttp::load().unwrap()) as Box<dyn OutboundAdapter>];

        let issuance_state = in_memory::issuance_state().await;
        let verification_state = in_memory::verification_state(test_verification_services(), outbound_adapters).await;

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
