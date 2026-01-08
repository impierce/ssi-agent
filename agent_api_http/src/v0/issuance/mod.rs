// Endpoint handlers
pub mod credential_configurations;
pub mod credential_issuer;
pub mod credentials;
pub mod nonce;
pub mod offers;

pub mod error;

use crate::v0::issuance::{
    credential_configurations::credential_configurations,
    credential_issuer::{
        credential::credential,
        credential_offer::credential_offer_uri,
        notification::notification,
        token_status_list::token_status_list,
        well_known::{
            oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer,
        },
    },
    credentials::{all_credentials, credentials, patch_credential},
    offers::{all_offers, offer, offers, send::send},
};
use crate::API_VERSION;
use agent_issuance::state::IssuanceState;
use axum::routing::get;
use axum::{routing::post, Router};
use std::sync::Arc;

pub fn router(issuance_state: Arc<IssuanceState>) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/credentials", post(credentials).get(all_credentials))
                .route(
                    "/credentials/{credential_id}",
                    get(credentials::credential).patch(patch_credential),
                )
                .route("/credential-configurations", post(credential_configurations))
                .route("/offers", post(offers).get(all_offers))
                .route("/offers/{offer_id}", get(offer))
                .route("/offers/send", post(send)),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/openid4vci/credential", post(credential))
        .route("/openid4vci/notification", post(notification))
        .route("/openid4vci/credential-offer/{offer_id}", get(credential_offer_uri))
        .route("/ietf-oauth-token-status-list/{path}", get(token_status_list))
        .with_state(issuance_state)
}
