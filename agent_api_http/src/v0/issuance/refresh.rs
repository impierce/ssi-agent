use crate::{error::IntoApiErrorExt, extractors::JsonBody};
use agent_issuance::{
    refresh::{service::refresh_credential as refresh, token::RefreshToken},
    state::IssuanceState,
};
use agent_library::state::LibraryState;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRefreshRequest {
    pub refresh_token: String,
}

// Answers a holder that presents the refresh token of its credential with an offer for a replacement credential. The
// token is the authorization, so no actor is involved. Kept out of the OpenAPI summary, as the other issuance routes.
#[utoipa::path(
    post,
    path = "/refresh-credential",
    request_body = CredentialRefreshRequest,
    operation_id = "refresh_credential",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Credential refresh continuation prepared successfully", body = String, content_type = "application/x-www-form-urlencoded"),
        (status = 403, description = "Credential refresh cannot proceed"),
        (status = 404, description = "Refresh reference not found"),
        (status = 500, description = "Credential refresh failed"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn refresh_credential(
    State(state): State<Arc<IssuanceState>>,
    Extension(library_state): Extension<Arc<LibraryState>>,
    JsonBody(CredentialRefreshRequest { refresh_token }): JsonBody<CredentialRefreshRequest>,
) -> Result<Response, ApiError> {
    let credential_offer = refresh(&state, &library_state, &RefreshToken::from(refresh_token))
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-www-form-urlencoded")],
        credential_offer,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{v0::issuance::router, API_VERSION};
    use agent_issuance::{
        application::credential_creation::{create_credential, CreateCredentialError},
        credential::{
            aggregate::{Credential, CredentialExpiry},
            command::CredentialCommand,
            entity::Data,
        },
        refresh::{
            preparation::{RefreshPreparation, RefreshPreparationError},
            service::RefreshError,
        },
        reissuance::service::{ReissuanceTrigger, Replacement},
        server_config::command::ServerConfigCommand,
        services::IssuanceServices,
        state::{initialize, issuance_state, SERVER_CONFIG_ID},
    };
    use agent_library::template::{
        aggregate::{Expiration, Status},
        command::TemplateCommand,
    };
    use agent_secret_manager::service::Service;
    use agent_shared::handlers::{public_command_handler, public_query_handler};
    use agent_store::in_memory::InMemory;
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use shared_kernel::authorization::Caller;
    use tower::ServiceExt;

    const ORIGINAL: &str = "11111111-1111-4111-8111-111111111111";
    const CONFIGURATION: &str = "SD-JWT VC";

    struct RefreshEverything;

    #[async_trait::async_trait]
    impl RefreshPreparation for RefreshEverything {
        async fn prepare(&self, _: &Credential, _: &str) -> Result<Replacement, RefreshPreparationError> {
            Ok(Replacement {
                credential_configuration_id: CONFIGURATION.to_owned(),
                credential: json!({"first_name": "Ferris"}),
                expires_at: Some(CredentialExpiry::Never),
                trigger: ReissuanceTrigger::default(),
            })
        }
    }

    /// An issuer with the hook installed and an issued, refreshable original, and the original's token.
    struct RefreshableOriginal {
        state: Arc<IssuanceState>,
        library_state: Arc<LibraryState>,
        token: String,
    }

    async fn refreshable_original() -> RefreshableOriginal {
        let mut state = issuance_state(&InMemory, IssuanceServices::default().await, &Default::default()).await;
        state.ports.refresh_preparation = Arc::new(RefreshEverything);
        initialize(&state).await.unwrap();
        public_command_handler(
            SERVER_CONFIG_ID,
            &state.command.server_config,
            ServerConfigCommand::UpdateCredentialConfiguration {
                credential_configuration: serde_json::from_value(json!({
                    "credential_configuration_id": CONFIGURATION,
                    "format": "dc+sd-jwt",
                    "refreshService": {"type": "VerifiableCredentialRefreshService2021"}
                }))
                .unwrap(),
                provisioned: false,
            },
        )
        .await
        .unwrap();
        let credential_configuration = public_query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await
            .unwrap()
            .unwrap()
            .credential_configurations[CONFIGURATION]
            .1
            .clone();
        create_credential(
            &state,
            Caller::Internal,
            ORIGINAL,
            CONFIGURATION,
            CredentialCommand::CreateUnsignedCredential {
                credential_id: ORIGINAL.to_owned(),
                data: Data {
                    raw: json!({"first_name": "Ferris"}),
                },
                credential_configuration: Box::new(credential_configuration),
                expires_at: CredentialExpiry::Never,
                refresh_token: None,
            },
        )
        .await
        .unwrap();
        public_command_handler(
            ORIGINAL,
            &state.command.credential,
            CredentialCommand::SignCredential {
                credential_id: ORIGINAL.to_owned(),
                subject_id: None,
                overwrite: false,
                proofs: None,
                status_list_id: "status-list".to_owned(),
                index: 0,
            },
        )
        .await
        .unwrap();
        let token = public_query_handler(ORIGINAL, &state.query.credential)
            .await
            .unwrap()
            .unwrap()
            .data
            .unwrap()
            .raw["refreshService"]["refreshToken"]
            .as_str()
            .unwrap()
            .to_owned();

        let library_state =
            agent_library::state::library_state(&InMemory, &Default::default(), Default::default()).await;
        public_command_handler(
            CONFIGURATION,
            &library_state.command.template,
            TemplateCommand::CreateNewTemplate {
                template_id: CONFIGURATION.to_owned(),
                source_template_id: None,
                title: CONFIGURATION.to_owned(),
                display: Box::new(None),
                data_model: Default::default(),
                holder_type: Default::default(),
                tags: None,
                status: Status::Published,
                visibility: Default::default(),
                credential_expiration: Some(Expiration::Never),
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

        RefreshableOriginal {
            state: Arc::new(state),
            library_state: Arc::new(library_state),
            token,
        }
    }

    #[tokio::test]
    async fn refresh_answers_with_the_form_url_encoded_offer() {
        let RefreshableOriginal {
            state,
            library_state,
            token,
        } = refreshable_original().await;

        let response = router((state.clone(), library_state))
            .oneshot(
                Request::post(format!("{API_VERSION}/refresh-credential"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "refreshToken": token }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let content_type = response.headers()[header::CONTENT_TYPE].clone();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let offers = public_query_handler("all_offers", &state.query.all_offers)
            .await
            .unwrap()
            .unwrap()
            .offers;
        let offer = offers
            .values()
            .next()
            .unwrap()
            .form_url_encoded_credential_offer
            .clone();
        assert_eq!(
            (status, content_type.to_str().unwrap()),
            (StatusCode::OK, "application/x-www-form-urlencoded")
        );
        assert_eq!(Some(String::from_utf8(body.to_vec()).unwrap()), offer);
    }

    #[tokio::test]
    async fn credential_responses_keep_no_refresh_token() {
        let RefreshableOriginal {
            state, library_state, ..
        } = refreshable_original().await;

        let response = router((state, library_state))
            .oneshot(
                Request::get(format!("{API_VERSION}/credentials/{ORIGINAL}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            (
                credential["data"]["raw"]["refreshService"].get("refreshToken"),
                &credential["signed"]
            ),
            (None, &serde_json::Value::Null)
        );
    }

    #[test]
    fn refresh_maps_its_failures_to_problem_types() {
        let problem = |error: ApiError| (error.status().as_u16(), error.type_url().map(str::to_owned));
        let issuance = |anchor: &str| Some(crate::error::type_url(&format!("issuance#{anchor}")));

        let actual = [
            problem(RefreshError::NotFound.into_api_error()),
            problem(RefreshError::Unavailable.into_api_error()),
            problem(RefreshError::PreparationFailed.into_api_error()),
            problem(RefreshError::Failed.into_api_error()),
            problem(CreateCredentialError::RefreshCapability.into_api_error()),
        ];

        assert_eq!(
            actual,
            [
                (404, issuance("refresh-reference-not-found")),
                (403, issuance("credential-refresh-unavailable")),
                (500, issuance("credential-refresh-preparation-failed")),
                (500, issuance("credential-refresh-failed")),
                (500, issuance("create-refresh-capability-failed")),
            ]
        );
    }
}
