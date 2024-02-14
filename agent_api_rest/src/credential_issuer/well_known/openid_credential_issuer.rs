use agent_issuance::{handlers::query_handler, state::ApplicationState};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::SERVER_CONFIG_ID;

#[axum_macros::debug_handler]
pub(crate) async fn openid_credential_issuer(State(state): State<ApplicationState>) -> impl IntoResponse {
    match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(view)) if view.credential_issuer_metadata.is_some() => {
            (StatusCode::OK, Json(view.credential_issuer_metadata)).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{app, tests::BASE_URL};

    use super::*;
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_shared::{config, UrlAppendHelpers};
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::{CredentialDefinition, JwtVcJson},
            CredentialFormats, Parameters,
        },
        credential_issuer::{
            credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
        },
        ProofType,
    };
    use serde_json::json;
    use tower::Service;

    pub async fn openid_credential_issuer(app: &mut Router) -> CredentialIssuerMetadata {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/.well-known/openid-credential-issuer")
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let credential_issuer_metadata: CredentialIssuerMetadata = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            credential_issuer_metadata,
            CredentialIssuerMetadata {
                credential_issuer: BASE_URL.clone(),
                authorization_server: None,
                credential_endpoint: BASE_URL.append_path_segment("openid4vci/credential"),
                batch_credential_endpoint: None,
                deferred_credential_endpoint: None,
                credentials_supported: vec![CredentialsSupportedObject {
                    id: None,
                    credential_format: CredentialFormats::JwtVcJson(Parameters {
                        format: JwtVcJson,
                        parameters: (
                            CredentialDefinition {
                                type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                                credential_subject: None,
                            },
                            None,
                        )
                            .into(),
                    }),
                    scope: None,
                    cryptographic_binding_methods_supported: Some(vec!["did:key".to_string()]),
                    cryptographic_suites_supported: Some(vec!["EdDSA".to_string()]),
                    proof_types_supported: Some(vec![ProofType::Jwt]),
                    display: Some(vec![json!({
                       "name": config!("credential_name").unwrap(),
                       "logo": {
                            "url": config!("credential_logo_url").unwrap()
                       }
                    })]),
                }],
                display: None,
            }
        );

        credential_issuer_metadata
    }

    #[tokio::test]
    async fn test_oauth_authorization_server_endpoint() {
        let state = in_memory::application_state().await;

        initialize(state.clone(), startup_commands(BASE_URL.clone())).await;

        let mut app = app(state);

        let _credential_issuer_metadata = openid_credential_issuer(&mut app).await;
    }
}
