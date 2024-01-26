mod credential_issuer;
mod credentials;
mod offers;

use agent_issuance::{model::aggregate::IssuanceData, queries::IssuanceDataView, state::ApplicationState};
use agent_shared::{config, ConfigError};
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
    let base_path = get_base_path();

    let path = |suffix: &str| -> String {
        if let Ok(base_path) = &base_path {
            format!("/{}{}", base_path, suffix)
        } else {
            suffix.to_string()
        }
    };

    Router::new()
        .route(&path("/v1/credentials"), post(credentials))
        .route(&path("/v1/offers"), post(offers))
        .route(
            &path("/.well-known/oauth-authorization-server"),
            get(oauth_authorization_server),
        )
        .route(
            &path("/.well-known/openid-credential-issuer"),
            get(openid_credential_issuer),
        )
        .route(&path("/auth/token"), post(token))
        .route(&path("/openid4vci/credential"), post(credential))
        .with_state(state)
}

fn get_base_path() -> Result<String, ConfigError> {
    config!("base_path").map(|mut base_path| {
        if base_path.starts_with('/') {
            base_path.remove(0);
        }

        if base_path.ends_with('/') {
            base_path.pop();
        }

        if base_path.is_empty() {
            panic!("AGENT_CONFIG_BASE_PATH can't be empty, remove or set path");
        }

        tracing::info!("Base path: {:?}", base_path);

        base_path
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::state::CQRS;
    use agent_issuance::{command::IssuanceCommand, services::IssuanceServices};
    use agent_store::in_memory;
    use serde_json::json;

    pub const PRE_AUTHORIZED_CODE: &str = "pre-authorized_code";
    pub const SUBJECT_ID: &str = "00000000-0000-0000-0000-000000000000";

    lazy_static::lazy_static! {
        pub static ref BASE_URL: url::Url = url::Url::parse("https://example.com").unwrap();
    }

    async fn handler() {}

    #[tokio::test]
    #[should_panic]
    async fn test_base_path_routes() {
        let state = in_memory::ApplicationState::new(vec![], IssuanceServices {}).await;

        std::env::set_var("AGENT_APPLICATION_BASE_PATH", "unicore");
        let router = app(state);

        let _ = router.route("/auth/token", post(handler));
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
