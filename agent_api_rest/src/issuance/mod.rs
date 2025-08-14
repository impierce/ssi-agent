pub mod credential_configurations;
pub mod credential_issuer;
pub mod credentials;
pub mod offers;

pub mod error;

use crate::issuance::{
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

pub fn router(issuance_state: IssuanceState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/credential-offer-page", get(credential_offer_page))
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

// FIXME: delete this!
/// This handler serves a simple HTML page with a button to trigger the credential offer flow on a mobile device.
async fn credential_offer_page() -> impl axum::response::IntoResponse {
    // The custom URL scheme link provided in your request.
    let offer_link = "openid-credential-offer://?credential_offer_uri=http%3A%2F%2F192.168.1.127%3A3033%2Fopenid4vci%2Fcredential-offer%2F001";

    // We create the HTML content directly in the function.
    // The `<a>` tag is styled to look like a button for a better user experience.
    let html_content = format!(
        r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Credential Offer</title>
    <style>
        body {{ font-family: sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }}
        .container {{ text-align: center; }}
        .offer-button {{
            display: inline-block;
            padding: 15px 25px;
            font-size: 24px;
            font-weight: bold;
            color: white;
            background-color: #007bff;
            border: none;
            border-radius: 5px;
            text-decoration: none;
            cursor: pointer;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>You have a new Credential Offer!</h1>
        <p>Click the button below to accept it in your wallet.</p>
        <a href="{}" class="offer-button">Accept Offer</a>
    </div>
</body>
</html>
"#,
        offer_link
    );

    axum::response::Html(html_content)
}
