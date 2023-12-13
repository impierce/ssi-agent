mod credential_issuer;
mod credentials;
mod offers;

use agent_issuance::{model::aggregate::IssuanceData, queries::IssuanceDataView, state::ApplicationState};
use axum::{
    routing::{get, post},
    Router,
};
use credential_issuer::{
    credential::credential,
    token::token,
    well_known::{
        oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer,
    },
};
use credentials::credentials;
use offers::offers;

// TODO: What to do with aggregate_id's?
pub const AGGREGATE_ID: &str = "agg-id-F39A0C";

pub fn app(state: ApplicationState<IssuanceData, IssuanceDataView>) -> Router {
    Router::new()
        .route("/v1/credentials", post(credentials))
        .route("/v1/offers", post(offers))
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/auth/token", post(token))
        .route("/openid4vci/credential", post(credential))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::command::IssuanceCommand;
    use oid4vci::credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    };
    use serde_json::json;

    pub const PRE_AUTHORIZED_CODE: &str = "pre-authorized_code";
    pub const SUBJECT_ID: &str = "00000000-0000-0000-0000-000000000000";
    lazy_static::lazy_static! {
        pub static ref BASE_URL: url::Url = url::Url::parse("https://example.com").unwrap();
    }

    pub async fn load_credential_format_template(state: ApplicationState<IssuanceData, IssuanceDataView>) {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::LoadCredentialFormatTemplate {
                    credential_format_template: serde_json::from_str(include_str!(
                        "../../agent_issuance/res/credential_format_templates/openbadges_v3.json"
                    ))
                    .unwrap(),
                },
                Default::default(),
            )
            .await
            .unwrap();
    }

    pub async fn load_authorization_server_metadata(state: ApplicationState<IssuanceData, IssuanceDataView>) {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::LoadAuthorizationServerMetadata {
                    authorization_server_metadata: Box::new(AuthorizationServerMetadata {
                        issuer: BASE_URL.clone(),
                        token_endpoint: Some(BASE_URL.join("auth/token").unwrap()),
                        ..Default::default()
                    }),
                },
                Default::default(),
            )
            .await
            .unwrap();
    }

    pub async fn load_credential_issuer_metadata(state: ApplicationState<IssuanceData, IssuanceDataView>) {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::LoadCredentialIssuerMetadata {
                    credential_issuer_metadata: CredentialIssuerMetadata {
                        credential_issuer: BASE_URL.clone(),
                        authorization_server: None,
                        credential_endpoint: BASE_URL.join("openid4vci/credential").unwrap(),
                        deferred_credential_endpoint: None,
                        batch_credential_endpoint: None,
                        credentials_supported: vec![],
                        display: None,
                    },
                },
                Default::default(),
            )
            .await
            .unwrap();
    }

    pub async fn create_credentials_supported(state: ApplicationState<IssuanceData, IssuanceDataView>) {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::CreateCredentialsSupported {
                    credentials_supported: vec![serde_json::from_value(json!({
                        "format": "jwt_vc_json",
                        "cryptographic_binding_methods_supported": [
                            "did:key",
                        ],
                        "cryptographic_suites_supported": [
                            "EdDSA"
                        ],
                        "credential_definition":{
                            "type": [
                                "VerifiableCredential",
                                "OpenBadgeCredential"
                            ]
                        },
                        "proof_types_supported": [
                            "jwt"
                        ]
                    }
                    ))
                    .unwrap()],
                },
                Default::default(),
            )
            .await
            .unwrap();
    }

    pub async fn create_unsigned_credential(state: ApplicationState<IssuanceData, IssuanceDataView>) -> String {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::CreateUnsignedCredential {
                    subject_id: SUBJECT_ID.to_string(),
                    credential: json!({
                        "credentialSubject": {
                            "first_name": "Ferris",
                            "last_name": "Rustacean"
                    }}),
                },
                Default::default(),
            )
            .await
            .unwrap();

        let view = state.load(AGGREGATE_ID).await.unwrap().unwrap();
        view.subjects
            .iter()
            .find(|subject| subject.id == SUBJECT_ID)
            .unwrap()
            .clone()
            .id
    }

    pub async fn create_credential_offer(state: ApplicationState<IssuanceData, IssuanceDataView>) {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::CreateCredentialOffer {
                    subject_id: SUBJECT_ID.to_string(),
                    pre_authorized_code: Some(PRE_AUTHORIZED_CODE.to_string()),
                },
                Default::default(),
            )
            .await
            .unwrap();
    }

    pub async fn create_token_response(state: ApplicationState<IssuanceData, IssuanceDataView>) -> String {
        state
            .execute_with_metadata(
                AGGREGATE_ID,
                IssuanceCommand::CreateTokenResponse {
                    token_request: oid4vci::token_request::TokenRequest::PreAuthorizedCode {
                        pre_authorized_code: PRE_AUTHORIZED_CODE.to_string(),
                        user_pin: None,
                    },
                },
                Default::default(),
            )
            .await
            .unwrap();

        let view = state.load(AGGREGATE_ID).await.unwrap().unwrap();

        view.subjects
            .iter()
            .find(|subject| subject.id == SUBJECT_ID)
            .unwrap()
            .clone()
            .token_response
            .unwrap()
            .access_token
            .clone()
    }
}
