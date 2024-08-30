pub mod credential_issuer;
pub mod credentials;
pub mod offers;

use agent_issuance::state::IssuanceState;
use axum::routing::get;
use axum::{routing::post, Router};

use crate::issuance::{
    credential_issuer::{
        credential::credential, token::token, well_known::oauth_authorization_server::oauth_authorization_server,
        well_known::openid_credential_issuer::openid_credential_issuer,
    },
    credentials::{credentials, get_credentials},
    offers::{offers, send::send},
};
use crate::API_VERSION;

pub fn router(issuance_state: IssuanceState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/credentials", post(credentials))
                .route("/credentials/:credential_id", get(get_credentials))
                .route("/offers", post(offers))
                .route("/offers/send", post(send)),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/auth/token", post(token))
        .route("/openid4vci/credential", post(credential))
        .with_state(issuance_state)
}
