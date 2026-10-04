use crate::{
    utils::StringifiedForm,
    v0::{issuance::error::PublicError, openapi::PROTOCOL_TAG},
};
use agent_authorization::application::{
    interactive_authorization_service::InteractiveAuthorizationService,
    pushed_authorization_service::PushedAuthorizationService,
};
use agent_authorization::state::AuthorizationState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use oid4vci::{
    authorization_request::AuthorizationRequest,
    errors::{OID4VCError, TokenErrorResponse},
    interactive_authorization_response::InteractiveAuthorizationResponse,
    wallet::PushedAuthorizationResponse,
    InteractiveAuthorizationFollowUpRequest, InteractiveAuthorizationRequest,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Serialize, Deserialize, Debug, Clone, utoipa::ToSchema)]
#[serde(untagged)]
pub enum AuthorizationRequestDto {
    InteractiveAuthorizationRequest(InteractiveAuthorizationRequest),
    FollowUpInteractiveAuthorizationRequest(InteractiveAuthorizationFollowUpRequest),
    PushedAuthorizationRequest(AuthorizationRequest),
}

/// Push an authorization request
///
/// Handles the Pushed Authorization Request (PAR) endpoint as defined by
/// [RFC 9126](https://www.rfc-editor.org/rfc/rfc9126.html), as well as the Interactive Authorization Request flow
/// as defined in OpenID4VCI 1.1.
///
/// Nested values, such as `authorization_details`, are JSON-encoded form values.
#[utoipa::path(
    post,
    path = "/auth/par",
    operation_id = "auth_par",
    tags = ["OAuth 2.0", PROTOCOL_TAG],
    request_body(content = AuthorizationRequestDto, content_type = "application/x-www-form-urlencoded"),
    responses(
        (
            status = 200,
            description = "Interactive authorization response to an initial or follow-up interactive authorization request",
            body = InteractiveAuthorizationResponse,
        ),
        (status = 201, description = "Pushed authorization response", body = PushedAuthorizationResponse),
        (
            status = 400,
            description = "The request body is not `application/x-www-form-urlencoded`, or the authorization request is invalid",
        ),
        (status = 401, description = "The client is invalid", body = OID4VCError<TokenErrorResponse>),
        (status = 422, description = "The request body is not a valid authorization request"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn par(
    State(state): State<Arc<AuthorizationState>>,
    StringifiedForm(authorization_request): StringifiedForm<AuthorizationRequestDto>,
) -> Result<Response, PublicError> {
    match authorization_request {
        AuthorizationRequestDto::InteractiveAuthorizationRequest(interactive_authorization_request) => {
            info!("Received interactive authorization request");

            let interactive_authorization_response =
                InteractiveAuthorizationService::handle_interactive_authorization_request(
                    &state,
                    interactive_authorization_request,
                )
                .await?;

            Ok((StatusCode::OK, Json(interactive_authorization_response)).into_response())
        }
        AuthorizationRequestDto::FollowUpInteractiveAuthorizationRequest(InteractiveAuthorizationFollowUpRequest {
            auth_session,
            openid4vp_response,
            code_verifier,
        }) => {
            info!("Received follow-up interactive authorization request for auth session: {auth_session}");

            let interactive_authorization_follow_up_response =
                InteractiveAuthorizationService::handle_interactive_authorization_request_follow_up(
                    &state,
                    auth_session,
                    openid4vp_response,
                    code_verifier,
                )
                .await?;

            Ok((StatusCode::OK, Json(interactive_authorization_follow_up_response)).into_response())
        }
        AuthorizationRequestDto::PushedAuthorizationRequest(pushed_authorization_request) => {
            info!(
                "Received pushed authorization request with state: {:?}",
                pushed_authorization_request.state
            );

            let authorization_response =
                PushedAuthorizationService::handle_pushed_authorization_request(&state, pushed_authorization_request)
                    .await?;

            Ok((StatusCode::CREATED, Json(authorization_response)).into_response())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::tests::TEMPLATE_ID;
    use crate::v0::issuance::router;
    use crate::v0::{
        authorization,
        issuance::{
            credentials::tests::{create_test_template_with_auth, credentials, setup_library_state},
            offers::tests::offers,
        },
    };
    use agent_authorization::{
        application::interactive_authorization_service::INTERACTION_TYPE_OPENID4VP,
        domain::oauth2_authorization_request::aggregate::test_utils::code_challenge,
        services::OAuth2AuthorizationRequestDomainServices, state::UNIME_REDIRECT_URI,
    };
    use agent_authorization::{services::AuthorizationServices, state::UNIME_CLIENT_ID};
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use agent_store::{authorization_state, in_memory::InMemory, issuance_state};
    use agent_verification::services::VerificationServices;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vc_core::utils::form_urlencoded::to_form_urlencoded_string;
    use oid4vci::{
        authorization_details::{AuthorizationDetailsObject, OpenidCredential},
        authorization_request::CodeChallengeMethod,
        credential_offer::AuthorizationCode,
        wallet::PushedAuthorizationResponse,
        InteractiveAuthorizationResponse, InteractiveAuthorizationStatus,
    };
    use serde_json::json;
    use tower::Service as _;
    use verification_authorization::VerificationAuthorizationAdapter;

    pub async fn par(app: &mut Router, issuer_state: String) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(AuthorizationRequest {
                            response_type: "code".to_string(),
                            state: Some("test_state".to_string()),
                            client_id: UNIME_CLIENT_ID.to_string(),
                            redirect_uri: Some(UNIME_REDIRECT_URI.parse().unwrap()),
                            code_challenge: Some(code_challenge()),
                            code_challenge_method: Some(CodeChallengeMethod::S256),
                            scope: Some("openid profile".to_string()),
                            issuer_state: Some(issuer_state),
                            authorization_details: Some(vec![AuthorizationDetailsObject {
                                r#type: OpenidCredential::Type,
                                locations: None,
                                credential_configuration_id: "configuration_id".to_string(),
                                credential_identifiers: None,
                                claims: None,
                            }]),
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let pushed_authorization_response: PushedAuthorizationResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(pushed_authorization_response.expires_in, 3600);

        pushed_authorization_response.request_uri
    }

    pub async fn interactive_authorization_request(
        app: &mut Router,
        issuer_state: String,
    ) -> InteractiveAuthorizationResponse {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(InteractiveAuthorizationRequest {
                            authorization_request: AuthorizationRequest {
                                response_type: "code".to_string(),
                                state: Some("test_state".to_string()),
                                client_id: UNIME_CLIENT_ID.to_string(),
                                redirect_uri: Some(UNIME_REDIRECT_URI.parse().unwrap()),
                                code_challenge: Some(code_challenge()),
                                code_challenge_method: Some(CodeChallengeMethod::S256),
                                scope: None,
                                issuer_state: Some(issuer_state),
                                authorization_details: Some(vec![AuthorizationDetailsObject {
                                    r#type: OpenidCredential::Type,
                                    locations: None,
                                    credential_configuration_id: "configuration_id".to_string(),
                                    credential_identifiers: None,
                                    claims: None,
                                }]),
                            },
                            interaction_types_supported: INTERACTION_TYPE_OPENID4VP.to_string(),
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let interactive_authorization_response: InteractiveAuthorizationResponse =
            serde_json::from_slice(&body).unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/auth/par")
                    .header(
                        http::header::CONTENT_TYPE,
                        mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                    )
                    .body(Body::from(
                        to_form_urlencoded_string(&json!(
                            AuthorizationRequestDto::FollowUpInteractiveAuthorizationRequest(
                                InteractiveAuthorizationFollowUpRequest {
                                    auth_session: interactive_authorization_response.auth_session.clone().unwrap(),
                                    openid4vp_response: Some(serde_json::json!({})),
                                    code_verifier: None,
                                },
                            )
                        ))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let interactive_authorization_response: InteractiveAuthorizationResponse =
            serde_json::from_slice(&body).unwrap();

        interactive_authorization_response
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_pushed_authorization_request_endpoint() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);

        agent_issuance::state::initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        create_test_template_with_auth(&library_state, false).await;

        let mut app = router((issuance_state.clone(), library_state));

        credentials(&mut app).await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, TEMPLATE_ID).await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &Default::default(),
                Default::default(),
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let _request_uri = par(&mut app, issuer_state).await;
    }

    // TODO: The holder functionality exists but is outdated and not easily integrated in this test structure. Once
    // holder state can be instantiated and initialized here, add holder service and state creation. Consider creating
    // a separate test module or helper that demonstrates holder functionality integration
    #[ignore = "Holder integration requires refactoring test infrastructure to support holder state alongside authorization state"]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_interactive_authorization_request_flow() {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);

        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let library_state = setup_library_state(&issuance_state).await;
        create_test_template_with_auth(&library_state, false).await;

        let mut app = router((issuance_state.clone(), library_state));

        credentials(&mut app).await;
        let (authorization_code, _pre_authorized_code) = offers(&mut app, TEMPLATE_ID).await.unwrap();
        let AuthorizationCode { issuer_state, .. } = authorization_code.unwrap();
        let issuer_state = issuer_state.unwrap();

        let verification_state = Arc::new(
            agent_store::verification_state(&InMemory, VerificationServices::default().await, &Default::default())
                .await,
        );

        let oauth2_authorization_request_domain_services = OAuth2AuthorizationRequestDomainServices::new(Box::new(
            VerificationAuthorizationAdapter::new(verification_state.clone()),
        ));

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &Default::default(),
                oauth2_authorization_request_domain_services,
            )
            .await,
        );
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();

        let mut app = authorization::router((authorization_state, issuance_state));

        let interactive_authorization_request = interactive_authorization_request(&mut app, issuer_state).await;

        assert_eq!(
            interactive_authorization_request.status,
            InteractiveAuthorizationStatus::Ok
        );
        // TODO: more field checks needed on the response
    }

    mod interactive {
        use super::*;
        use agent_authorization::services::OpenId4VpPresentationService;
        use serde_json::Value;
        use tower::ServiceExt as _;

        async fn app() -> Router {
            app_with(|verification_state| Box::new(VerificationAuthorizationAdapter::new(verification_state))).await
        }

        async fn app_with(
            presentation_service: impl FnOnce(
                Arc<agent_verification::state::VerificationState>,
            ) -> Box<dyn OpenId4VpPresentationService>,
        ) -> Router {
            let issuance_state = Arc::new(
                issuance_state(
                    &InMemory,
                    IssuanceServices::default().await,
                    &Default::default(),
                    Default::default(),
                )
                .await,
            );
            let verification_state = Arc::new(
                agent_store::verification_state(
                    &InMemory,
                    VerificationServices::default().await,
                    &Default::default(),
                    Default::default(),
                )
                .await,
            );
            let authorization_state = Arc::new(
                authorization_state(
                    &InMemory,
                    AuthorizationServices::default().await,
                    &Default::default(),
                    Default::default(),
                    OAuth2AuthorizationRequestDomainServices::new(presentation_service(verification_state)),
                )
                .await,
            );
            agent_authorization::state::initialize(&authorization_state)
                .await
                .unwrap();

            authorization::router((authorization_state, issuance_state))
        }

        fn authorization_request() -> AuthorizationRequest {
            AuthorizationRequest {
                response_type: "code".to_string(),
                state: Some("test_state".to_string()),
                client_id: UNIME_CLIENT_ID.to_string(),
                redirect_uri: Some(UNIME_REDIRECT_URI.parse().unwrap()),
                code_challenge: Some(code_challenge()),
                code_challenge_method: Some(CodeChallengeMethod::S256),
                scope: None,
                issuer_state: None,
                authorization_details: None,
            }
        }

        fn interactive_request(authorization_request: AuthorizationRequest) -> Value {
            json!(InteractiveAuthorizationRequest {
                authorization_request,
                interaction_types_supported: INTERACTION_TYPE_OPENID4VP.to_string(),
            })
        }

        async fn post_par(app: &Router, body: &Value) -> (StatusCode, Value) {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/auth/par")
                        .header(
                            http::header::CONTENT_TYPE,
                            mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
                        )
                        .body(Body::from(to_form_urlencoded_string(body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

            (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
        }

        #[tokio::test]
        async fn an_interactive_authorization_request_requires_an_openid4vp_presentation() {
            let app = app().await;

            let (status, body) = post_par(&app, &interactive_request(authorization_request())).await;

            assert_eq!(status, StatusCode::OK, "{body}");
            let response: InteractiveAuthorizationResponse = serde_json::from_value(body).unwrap();
            assert_eq!(response.status, InteractiveAuthorizationStatus::RequireInteraction);
            assert!(response.auth_session.is_some());
            assert!(response.openid4vp_request.is_some());
            assert!(response.code.is_none());
        }

        #[tokio::test]
        async fn invalid_interactive_authorization_requests_are_rejected() {
            let app = app().await;

            let unsupported_interaction_type = json!(InteractiveAuthorizationRequest {
                authorization_request: authorization_request(),
                interaction_types_supported: "urn:example:unsupported".to_string(),
            });
            let unknown_redirect_uri = interactive_request(AuthorizationRequest {
                redirect_uri: Some("unime://other".parse().unwrap()),
                ..authorization_request()
            });
            let missing_redirect_uri = interactive_request(AuthorizationRequest {
                redirect_uri: None,
                ..authorization_request()
            });
            let unsupported_response_type = interactive_request(AuthorizationRequest {
                response_type: "token".to_string(),
                ..authorization_request()
            });
            let missing_code_challenge_method = interactive_request(AuthorizationRequest {
                code_challenge_method: None,
                ..authorization_request()
            });
            let unsupported_code_challenge_method = interactive_request(AuthorizationRequest {
                code_challenge_method: Some(CodeChallengeMethod::Plain),
                ..authorization_request()
            });

            for (case, body) in [
                ("unsupported interaction type", unsupported_interaction_type),
                ("unknown redirect URI", unknown_redirect_uri),
                ("missing redirect URI", missing_redirect_uri),
                ("unsupported response type", unsupported_response_type),
                ("missing code challenge method", missing_code_challenge_method),
                ("unsupported code challenge method", unsupported_code_challenge_method),
            ] {
                let (status, body) = post_par(&app, &body).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {body}");
                assert_eq!(body["error"], "invalid_request", "{case}");
            }
        }

        #[tokio::test]
        async fn follow_up_requests_need_a_known_session_and_a_valid_presentation() {
            let app = app().await;
            let (_, body) = post_par(&app, &interactive_request(authorization_request())).await;
            let auth_session = body["auth_session"].as_str().unwrap().to_string();

            let unknown_session = json!(InteractiveAuthorizationFollowUpRequest {
                auth_session: "urn:uuid:00000000-0000-0000-0000-000000000000".to_string(),
                openid4vp_response: Some(json!({})),
                code_verifier: None,
            });
            let missing_presentation = json!({ "auth_session": auth_session });
            let invalid_presentation = json!(InteractiveAuthorizationFollowUpRequest {
                auth_session,
                openid4vp_response: Some(json!({})),
                code_verifier: None,
            });

            for (case, body) in [
                ("unknown session", unknown_session),
                ("missing presentation", missing_presentation),
                ("invalid presentation", invalid_presentation),
            ] {
                let (status, body) = post_par(&app, &body).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {body}");
                assert_eq!(body["error"], "invalid_request", "{case}");
            }
        }

        #[tokio::test]
        async fn a_verified_presentation_completes_the_interactive_authorization_with_a_code() {
            /// Creates real OpenID4VP requests, but accepts only the presentation `{"vp_token": "verified"}`.
            struct AcceptingPresentationService(VerificationAuthorizationAdapter);

            #[async_trait::async_trait]
            impl OpenId4VpPresentationService for AcceptingPresentationService {
                async fn create_openid4vp_presentation_request(&self, state: String) -> anyhow::Result<Value> {
                    self.0.create_openid4vp_presentation_request(state).await
                }

                async fn verify_openid4vp_response(&self, openid4vp_response: Value) -> anyhow::Result<()> {
                    anyhow::ensure!(openid4vp_response == json!({ "vp_token": "verified" }));
                    Ok(())
                }
            }

            let app = app_with(|verification_state| {
                Box::new(AcceptingPresentationService(VerificationAuthorizationAdapter::new(
                    verification_state,
                )))
            })
            .await;
            let (_, body) = post_par(&app, &interactive_request(authorization_request())).await;
            let auth_session = body["auth_session"].as_str().unwrap().to_string();

            let (status, body) = post_par(
                &app,
                &json!(AuthorizationRequestDto::FollowUpInteractiveAuthorizationRequest(
                    InteractiveAuthorizationFollowUpRequest {
                        auth_session,
                        openid4vp_response: Some(json!({ "vp_token": "verified" })),
                        code_verifier: None,
                    }
                )),
            )
            .await;

            assert_eq!(status, StatusCode::OK, "{body}");
            let response: InteractiveAuthorizationResponse = serde_json::from_value(body).unwrap();
            assert_eq!(response.status, InteractiveAuthorizationStatus::Ok);
            assert!(response.code.is_some());
            assert_eq!(response.expires_in, Some(600));
            assert!(response.auth_session.is_none());
            assert!(response.openid4vp_request.is_none());
        }
    }
}
