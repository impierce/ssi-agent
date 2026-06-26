// Endpoint handlers
pub mod credential_issuer;
pub mod credentials;
pub mod ietf_oauth_sd_jwt_vc;
pub mod nonce;
pub mod offers;
pub mod openapi;
pub mod public_offers;

pub mod error;

use crate::v0::issuance::{
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
    nonce::nonce,
    offers::{
        all_offers, offer, offers,
        send::{individual_offer, organization_offer},
    },
    public_offers::{
        all_public_offers, create_public_offer, delete_public_offer, take_public_offer_offline,
        take_public_offer_online,
    },
};
use crate::API_VERSION;
use agent_issuance::state::IssuanceState;
use agent_library::state::LibraryState;
use axum::routing::get;
use axum::{routing::post, Router};
use std::sync::Arc;

pub fn router((issuance_state, library_state): (Arc<IssuanceState>, Arc<LibraryState>)) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/credentials", post(credentials).get(all_credentials))
                .route(
                    "/credentials/{credential_id}",
                    get(credentials::credential).patch(patch_credential),
                )
                .route("/offers", post(offers).get(all_offers))
                .route("/offers/{offer_id}", get(offer))
                .route("/offers/send-offer-to-individual", post(individual_offer))
                .route("/offers/send-offer-to-organization", post(organization_offer))
                // Public offers
                .route("/get-all-public-offers", get(all_public_offers))
                .route("/create-public-offer", post(create_public_offer))
                .route("/take-public-offer-offline", post(take_public_offer_offline))
                .route("/take-public-offer-online", post(take_public_offer_online))
                .route("/delete-public-offer", post(delete_public_offer))
                .with_state(issuance_state.clone())
                .layer(axum::Extension(library_state.clone())),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server),
        )
        .route("/.well-known/openid-credential-issuer", get(openid_credential_issuer))
        .route("/openid4vci/credential", post(credential))
        .route("/openid4vci/nonce", post(nonce))
        .route("/openid4vci/notification", post(notification))
        .route("/openid4vci/credential-offer/{offer_id}", get(credential_offer_uri))
        .route("/ietf-oauth-token-status-list/{path}", get(token_status_list))
        // TODO: Move this route to `../library` once `agent_library` is properly implemented.
        .route(
            "/vct/{credential_configuration_id}/{version}",
            get(ietf_oauth_sd_jwt_vc::type_metadata),
        )
        .with_state(issuance_state)
}
