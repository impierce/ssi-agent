pub mod credential_issuer;
pub mod credentials;
pub mod offers;

pub mod error;

use agent_issuance::state::{IssuanceState, SERVER_CONFIG_ID};
use axum::routing::get;
use axum::{routing::post, Router};
use credential_issuer::credential_offer::credential_offer_uri;
use credentials::all_credentials;
use http_api_problem::ApiError;
use hyper::StatusCode;
use offers::{all_offers, offer};
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;

use crate::handlers::query_handler;
use crate::issuance::{
    credential_issuer::{
        credential::credential, token::token, well_known::oauth_authorization_server::oauth_authorization_server,
        well_known::openid_credential_issuer::openid_credential_issuer,
    },
    credentials::credentials,
    offers::{offers, send::send},
};
use crate::API_VERSION;

pub fn router(issuance_state: IssuanceState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/credentials", post(credentials).get(all_credentials))
                .route("/credentials/{credential_id}", get(credentials::credential))
                .route("/offers", post(offers).get(all_offers))
                .route("/offers/{offer_id}", get(offer))
                .route("/offers/send", post(send)),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/auth/token", post(token))
        .route("/openid4vci/credential", post(credential))
        .route("/openid4vci/credential-offer/{offer_id}", get(credential_offer_uri))
        .with_state(issuance_state)
}

pub(crate) async fn query_credential_issuer_metadata(
    state: &IssuanceState,
) -> Result<CredentialIssuerMetadata, ApiError> {
    // Get the `CredentialIssuerMetadata` from the `ServerConfigView`.
    let credential_issuer_metadata = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
            .await?
            .and_then(|server_config_view| server_config_view.credential_issuer_metadata)
            .ok_or_else(|| {
                ApiError::builder(StatusCode::INTERNAL_SERVER_ERROR)
                    .title("Impossible Server State")
                    .message("CredentialIssuerMetadata is missing from ServerConfigView. This indicates an initialization failure that should never occur.")
                    .finish()
            })?;

    Ok(credential_issuer_metadata)
}
