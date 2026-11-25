use crate::error::IntoApiErrorExt;
use crate::extractors::{JsonBody, PathParams, RequestActor};
use crate::handlers::{caller, query_handler};
use agent_issuance::{
    credential::aggregate::CredentialExpiry,
    reissuance::{
        service::{reissue_credential, PreparedReissuance, ReissuanceTrigger, Replacement},
        views::ReissuanceView,
    },
    state::IssuanceState,
};
use agent_library::state::LibraryState;
use axum::Extension;
use axum::{
    extract::{Json, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCredentialReissuanceRequest {
    pub original_credential_id: String,
    pub credential_configuration_id: String,
    pub credential: serde_json::Value,
    #[serde(default)]
    pub expires_at: Option<CredentialExpiry>,
    pub reason: Option<String>,
    // TODO stronger types
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
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
    request_body = CreateCredentialReissuanceRequest,
    operation_id = "reissue_credential",
    tags = ["Issuance"],
    responses(
        (status = 201, description = "Credential reissuance prepared successfully", body = CreateCredentialReissuanceResponse),
        (status = 400, description = "Malformed JSON request body, the original credential has not been issued, or the credential payload or configuration does not fit the original credential"),
        (status = 403, description = "The reissuance policy does not allow this reissuance"),
        (status = 404, description = "Original credential or credential configuration not found"),
        (status = 409, description = "A reissuance of the original credential already exists"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_reissuances(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Extension(library_state): Extension<Arc<LibraryState>>,
    JsonBody(CreateCredentialReissuanceRequest {
        original_credential_id,
        credential_configuration_id,
        credential,
        expires_at,
        reason,
        trigger_type,
        triggered_by,
    }): JsonBody<CreateCredentialReissuanceRequest>,
) -> Result<Response, ApiError> {
    let replacement = Replacement {
        credential_configuration_id,
        credential,
        expires_at,
        trigger: ReissuanceTrigger {
            reason,
            trigger_type,
            triggered_by,
        },
    };
    let PreparedReissuance {
        relation,
        credential_offer,
    } = reissue_credential(
        &state,
        &library_state,
        caller(actor),
        &original_credential_id,
        replacement,
    )
    .await
    .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateCredentialReissuanceResponse {
            id: relation.reissuance_id,
            original_credential_id: relation.original_credential_id,
            new_credential_id: relation.new_credential_id,
            offer_id: relation.offer_id,
            credential_configuration_id: relation.credential_configuration_id,
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
pub(crate) async fn all_credential_reissuances(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let reissuances = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_reissuances",
        None,
        &state.query.all_reissuances,
    )
    .await?
    .map(|all_reissuances_view| all_reissuances_view.reissuances.into_values().collect::<Vec<_>>())
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(reissuances)).into_response())
}

#[utoipa::path(
    get,
    path = "/get-credential-reissuance/{id}",
    operation_id = "get_credential_reissuance",
    params(("id" = String, Path, description = "Reissuance ID")),
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Credential reissuance relation retrieved successfully", body = ReissuanceView),
        (status = 404, description = "Credential reissuance relation not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn credential_reissuance(
    State(state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    PathParams(reissuance_id): PathParams<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &reissuance_id,
        Some(&reissuance_id),
        &state.query.reissuance,
    )
    .await?
    .map(|reissuance_view| (StatusCode::OK, Json(reissuance_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tests::{holder_did, key_proof},
        v0::{
            authorization,
            authorization::authorization_server::token::tests::token,
            issuance::{credential_issuer::credential::tests::TEST_NONCE, router},
        },
        API_VERSION,
    };
    use agent_authorization::services::{AuthorizationServices, OAuth2AuthorizationRequestDomainServices};
    use agent_authorization::state::authorization_state;
    use agent_issuance::{
        credential::{aggregate::Status as CredentialStatus, command::CredentialCommand, entity::Data},
        nonce::command::NonceCommand,
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, issuance_state, SERVER_CONFIG_ID},
    };
    use agent_secret_manager::service::Service;
    use agent_shared::{
        config::CredentialConfiguration,
        handlers::{public_command_handler as command_handler, public_query_handler as query_handler},
    };
    use agent_store::in_memory::InMemory;
    use axum::{
        body::{self, Body},
        http::{self, header, Method, Request},
    };
    use oid4vci::credential_offer::AuthorizationCode;
    use serde_json::{json, Value};
    use serial_test::serial;
    use tower::ServiceExt;

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await);
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
            "11111111-1111-4111-8111-111111111111",
            &state.command.credential,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: "11111111-1111-4111-8111-111111111111".to_string(),
                data: Data {
                    raw: json!({
                        "first_name": "Ferris",
                        "last_name": "Rustacean",
                        "dob": "2010-01-01"
                    }),
                },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
                refresh_token: None,
            },
        )
        .await
        .unwrap();
        command_handler(
            "11111111-1111-4111-8111-111111111111",
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: "11111111-1111-4111-8111-111111111111".to_owned(),
                subject_id: None,
                overwrite: false,
                proofs: None,
                status_list_id: "status-list".to_owned(),
                index: 0,
            },
        )
        .await
        .unwrap();
    }

    async fn test_library_state() -> Arc<LibraryState> {
        let state =
            Arc::new(agent_library::state::library_state(&InMemory, &Default::default(), Default::default()).await);
        command_handler(
            "SD-JWT VC",
            &state.command.template,
            agent_library::template::command::TemplateCommand::CreateNewTemplate {
                template_id: "SD-JWT VC".to_owned(),
                source_template_id: None,
                title: "SD-JWT VC".to_owned(),
                display: Box::new(None),
                data_model: Default::default(),
                holder_type: Default::default(),
                tags: None,
                status: agent_library::template::aggregate::Status::Published,
                visibility: Default::default(),
                credential_expiration: Some(agent_library::template::aggregate::Expiration::Never),
                description: None,
                r#type: vec![],
                schema: Box::new(None),
                schema_properties_attributes: None,
                holder_authorization: Default::default(),
                origin: None,
            },
        )
        .await
        .unwrap();
        state
    }

    async fn post_reissuance(state: Arc<IssuanceState>) -> (StatusCode, Value) {
        let app = router((state, test_library_state().await));
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/reissue-credential"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "originalCredentialId": "11111111-1111-4111-8111-111111111111",
                            "credentialConfigurationId": "SD-JWT VC",
                            "credential": {
                                "first_name": "Ferris",
                                "last_name": "Reissued",
                                "dob": "2010-01-01"
                            },
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
    async fn reissue_credential_prepares_an_offer_and_relation() {
        let state = test_state().await;
        create_original_credential(&state).await;

        let (status, body) = post_reissuance(state.clone()).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["originalCredentialId"], "11111111-1111-4111-8111-111111111111");
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

        assert_eq!(
            reissuance.original_credential_id,
            "11111111-1111-4111-8111-111111111111"
        );
        assert_eq!(reissuance.new_credential_id, new_credential_id);
        assert_eq!(reissuance.offer_id, offer_id);
        assert_eq!(new_credential.data.unwrap().raw["last_name"], json!("Reissued"));
        assert_eq!(offer.credential_ids, vec![new_credential_id.to_string()]);
    }

    #[tokio::test]
    #[serial]
    async fn a_reissued_credential_can_be_issued_through_oid4vci() {
        let state = test_state().await;
        create_original_credential(&state).await;

        let (status, body) = post_reissuance(state.clone()).await;

        assert_eq!(status, StatusCode::CREATED);
        let offer_id = body["offerId"].as_str().unwrap();
        let new_credential_id = body["newCredentialId"].as_str().unwrap();

        let authorization_state = Arc::new(
            authorization_state(
                &InMemory,
                AuthorizationServices::default().await,
                &Default::default(),
                OAuth2AuthorizationRequestDomainServices::default(),
            )
            .await,
        );
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

        let proof = key_proof("did:key", Some(holder_did("did:key").await), TEST_NONCE).await;
        let app = router((state.clone(), test_library_state().await));
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
                                "jwt": [proof]
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
    async fn list_all_credential_reissuances_returns_relations() {
        let state = test_state().await;
        create_original_credential(&state).await;
        let (status, created_body) = post_reissuance(state.clone()).await;
        assert_eq!(status, StatusCode::CREATED);

        let app = router((state, test_library_state().await));
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
        assert_eq!(
            body[0]["original_credential_id"],
            "11111111-1111-4111-8111-111111111111"
        );
    }

    #[tokio::test]
    async fn get_credential_reissuance_returns_the_relation() {
        let state = test_state().await;
        create_original_credential(&state).await;
        let (status, created_body) = post_reissuance(state.clone()).await;
        assert_eq!(status, StatusCode::CREATED);

        let reissuance_id = created_body["id"].as_str().unwrap();
        let app = router((state, test_library_state().await));
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
        assert_eq!(body["original_credential_id"], "11111111-1111-4111-8111-111111111111");
        assert_eq!(body["new_credential_id"], created_body["newCredentialId"]);
    }

    #[tokio::test]
    async fn get_credential_reissuance_answers_not_found_for_an_unknown_id() {
        let state = test_state().await;
        let app = router((state, test_library_state().await));

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

    #[tokio::test]
    async fn reissuance_routes_reject_an_unauthorized_actor() {
        use shared_kernel::authorization::{
            Actor, AuthorizationChecker, AuthorizationError, AuthorizationRequest, Caller,
        };

        struct DenyActor;

        #[async_trait::async_trait]
        impl AuthorizationChecker for DenyActor {
            async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
                assert_eq!(request.caller, Caller::Actor(Actor::user("denied-user")));
                Err(AuthorizationError::Forbidden)
            }
        }

        let mut state = test_state().await;
        create_original_credential(&state).await;
        Arc::get_mut(&mut state).unwrap().authorization_checker = Arc::new(DenyActor);
        let library = test_library_state().await;
        let app = router((state.clone(), library));

        for (method, path, body) in [
            (Method::GET, "/list-all-credential-reissuances", Body::empty()),
            (Method::GET, "/get-credential-reissuance/reissuance-id", Body::empty()),
            (
                Method::POST,
                "/reissue-credential",
                Body::from(
                    json!({
                        "originalCredentialId": "11111111-1111-4111-8111-111111111111",
                        "credentialConfigurationId": "SD-JWT VC",
                        "credential": {},
                        "expiresAt": "never"
                    })
                    .to_string(),
                ),
            ),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(format!("{API_VERSION}{path}"))
                .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(body)
                .unwrap();
            request.extensions_mut().insert(Actor::user("denied-user"));
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }

        assert!(query_handler("all_reissuances", &state.query.all_reissuances)
            .await
            .unwrap()
            .is_none());
        assert!(query_handler("all_offers", &state.query.all_offers)
            .await
            .unwrap()
            .is_none());
    }
    #[test]
    fn reissue_credential_rejects_an_unsupported_status_action() {
        let error = serde_json::from_value::<CreateCredentialReissuanceRequest>(json!({
            "originalCredentialId": "original", "credentialConfigurationId": "SD-JWT VC", "credential": {}, "expiresAt": "never", "statusAction": "revoked"
        })).unwrap_err();
        assert!(error.to_string().contains("statusAction"));
    }

    #[test]
    fn reissuance_errors_keep_their_problem_types_and_statuses() {
        use agent_issuance::{
            application::template_issuance::TemplateIssuanceError,
            credential::error::CredentialError,
            reissuance::{error::ReissuanceError, policy::ReissuancePolicyError},
        };
        use cqrs_es::persist::PersistenceError;
        use shared_kernel::authorization::AuthorizationError;

        let cases = [
            (
                ReissuanceError::OriginalCredentialNotFound("original".to_owned()),
                404,
                "issuance#original-credential-not-found",
            ),
            (
                ReissuanceError::OriginalCredentialNotIssued,
                400,
                "issuance#original-credential-not-issued",
            ),
            (
                ReissuanceError::CredentialConfigurationNotFound("configuration".to_owned()),
                404,
                "issuance#no-credential-configuration-found",
            ),
            (
                ReissuanceError::CredentialConfigurationMismatch,
                400,
                "issuance#credential-configuration-mismatch",
            ),
            (
                ReissuanceError::UnsupportedCredentialFormat(json!("jwt_vc_json")),
                400,
                "issuance#unsupported-credential-format",
            ),
            (
                ReissuanceError::InvalidCredentialPayload,
                400,
                "issuance#invalid-credential-type",
            ),
            (
                ReissuanceError::Template(TemplateIssuanceError::TemplateNotPublished),
                422,
                "issuance#template-not-published",
            ),
            (
                ReissuanceError::NotAllowed(ReissuancePolicyError::NotAllowed),
                403,
                "issuance#reissuance-not-allowed",
            ),
            (
                ReissuanceError::ReplacementRejected(CredentialError::InvalidCredentialPayloadError(
                    agent_library::json_schema_validation::JsonSchemaError::InvalidJsonData("invalid".to_owned()),
                )),
                400,
                "issuance#invalid-credential-payload",
            ),
            (
                ReissuanceError::AlreadyExists,
                409,
                "issuance#reissuance-already-exists",
            ),
            (
                ReissuanceError::Authorization(AuthorizationError::Forbidden),
                403,
                "authorization#forbidden",
            ),
            (
                ReissuanceError::Persistence(PersistenceError::OptimisticLockError),
                503,
                "persistence#aggregate-conflict",
            ),
            (
                ReissuanceError::Failed("offer refused".to_owned()),
                500,
                "issuance#credential-reissuance-failed",
            ),
        ];

        let (actual, expected): (Vec<_>, Vec<_>) = cases
            .into_iter()
            .map(|(error, status, anchor)| {
                let problem = error.into_api_error();
                (
                    (problem.status().as_u16(), problem.type_url().map(str::to_owned)),
                    (status, Some(crate::error::type_url(anchor))),
                )
            })
            .unzip();

        assert_eq!(actual, expected);
    }
}
