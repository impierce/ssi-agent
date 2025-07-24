use oid4vci::authorization_details::CredentialConfigurationOrFormat;

// src/web/templates.rs (or directly in main.rs for a small example)
use askama::Template;

#[derive(Template)]
#[template(path = "login.html")] // This tells Askama where to find the template file
pub struct LoginPageTemplate {
    pub title: String,
    pub client_id: String, // Client ID of the OAuth2 client
    pub request_uri: String,
}

#[derive(Template)]
#[template(path = "consent.html")] // <-- New template struct
pub struct ConsentPageTemplate {
    pub title: String,
    pub client_name: String,
    pub client_id: String,
    pub scope: String, // Space-separated string
    pub authorization_details: Vec<AuthorizationDetailsObject>,
    pub request_uri: Uuid, // To restore original OAuth context after consent
}

// You might create a helper type for Axum for cleaner handlers
// src/web/mod.rs or src/main.rs
pub struct HtmlTemplate<T>(pub T);

// We need to implement IntoResponse for `HtmlTemplate<T>`
// so that we can return it directly from Axum handlers.
use axum::response::{Html, IntoResponse, Response};
use http::StatusCode;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use uuid::Uuid;

impl<T: Template> IntoResponse for HtmlTemplate<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template: {}", err),
            )
                .into_response(),
        }
    }
}
