mod credential_issuer;
mod credentials;
mod offers;

use std::sync::Arc;

use agent_issuance::{
    credential::{aggregate::Credential, queries::CredentialView},
    offer::{aggregate::Offer, queries::OfferView},
    // model::aggregate::IssuanceData,
    // queries::IssuanceDataView,
    server_config::{aggregate::ServerConfig, command::ServerConfigCommand, queries::ServerConfigView},
    state::{self, ApplicationState, Domain, CQRS},
};
use axum::{
    extract::FromRef,
    routing::{get, post},
    Router,
};
use cqrs_es::{Aggregate, View};
use credential_issuer::{
    credential::credential,
    token::token,
    well_known::{
        oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer,
    },
};
use credentials::credentials;
use offers::offers;
use oid4vci::credential_issuer::authorization_server_metadata::AuthorizationServerMetadata;
use serde_json::json;

#[derive(Clone)]
pub(crate) struct AggregateHandler<D: Domain>(pub(crate) state::AggregateHandler<D>);

impl<D: Domain> std::ops::Deref for AggregateHandler<D> {
    type Target = state::AggregateHandler<D>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRef<ApplicationState> for AggregateHandler<Offer> {
    fn from_ref(application_state: &ApplicationState) -> AggregateHandler<Offer> {
        AggregateHandler(application_state.offer.clone())
    }
}

impl FromRef<ApplicationState> for AggregateHandler<Credential> {
    fn from_ref(application_state: &ApplicationState) -> AggregateHandler<Credential> {
        AggregateHandler(application_state.credential.clone())
    }
}

impl FromRef<ApplicationState> for AggregateHandler<ServerConfig> {
    fn from_ref(application_state: &ApplicationState) -> AggregateHandler<ServerConfig> {
        AggregateHandler(application_state.server_config.clone())
    }
}

// TODO: What to do with aggregate_id's?
// pub const AGGREGATE_ID: &str = "agg-id-F39A0C";

// #[axum_macros::debug_handler]
pub fn app(app_state: ApplicationState) -> Router {
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
        .with_state(app_state)
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use agent_issuance::command::IssuanceCommand;
//     use serde_json::json;

//     pub const PRE_AUTHORIZED_CODE: &str = "pre-authorized_code";
//     pub const SUBJECT_ID: &str = "00000000-0000-0000-0000-000000000000";
//     lazy_static::lazy_static! {
//         pub static ref BASE_URL: url::Url = url::Url::parse("https://example.com").unwrap();
//     }

//     pub async fn create_unsigned_credential(state: ApplicationState<IssuanceData, IssuanceDataView>) -> String {
//         state
//             .execute_with_metadata(
//                 AGGREGATE_ID,
//                 IssuanceCommand::CreateUnsignedCredential {
//                     subject_id: SUBJECT_ID.to_string(),
//                     credential: json!({
//                         "credentialSubject": {
//                             "first_name": "Ferris",
//                             "last_name": "Rustacean"
//                     }}),
//                 },
//                 Default::default(),
//             )
//             .await
//             .unwrap();

//         let view = state.load(AGGREGATE_ID).await.unwrap().unwrap();
//         view.subjects
//             .iter()
//             .find(|subject| subject.id == SUBJECT_ID)
//             .unwrap()
//             .clone()
//             .id
//     }

//     pub async fn create_credential_offer(state: ApplicationState<IssuanceData, IssuanceDataView>) {
//         state
//             .execute_with_metadata(
//                 AGGREGATE_ID,
//                 IssuanceCommand::CreateCredentialOffer {
//                     subject_id: SUBJECT_ID.to_string(),
//                     pre_authorized_code: Some(PRE_AUTHORIZED_CODE.to_string()),
//                 },
//                 Default::default(),
//             )
//             .await
//             .unwrap();
//     }

//     pub async fn create_token_response(state: ApplicationState<IssuanceData, IssuanceDataView>) -> String {
//         state
//             .execute_with_metadata(
//                 AGGREGATE_ID,
//                 IssuanceCommand::CreateTokenResponse {
//                     token_request: oid4vci::token_request::TokenRequest::PreAuthorizedCode {
//                         pre_authorized_code: PRE_AUTHORIZED_CODE.to_string(),
//                         user_pin: None,
//                     },
//                 },
//                 Default::default(),
//             )
//             .await
//             .unwrap();

//         let view = state.load(AGGREGATE_ID).await.unwrap().unwrap();

//         view.subjects
//             .iter()
//             .find(|subject| subject.id == SUBJECT_ID)
//             .unwrap()
//             .clone()
//             .token_response
//             .unwrap()
//             .access_token
//             .clone()
//     }
// }
