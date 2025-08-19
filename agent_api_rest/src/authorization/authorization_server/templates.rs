use askama::Template;
use axum::response::{Html, IntoResponse, Response};
use http::StatusCode;
use oid4vci::authorization_details::AuthorizationDetailsObject;
use oid4vci::authorization_details::CredentialConfigurationOrFormat;
use uuid::Uuid;

/// Represents the view model for the consent page.
#[derive(Template)]
#[template(path = "consent.html")]
pub struct ConsentPageTemplate {
    pub client_name: String,
    pub client_id: String,
    pub scope: String,
    pub authorization_details: Vec<AuthorizationDetailsObject>,
    pub request_uri: Uuid,
}

/// Wrapper for HTML templates to implement the `IntoResponse` trait
pub struct HtmlTemplate<T>(pub T);

impl<T: Template> IntoResponse for HtmlTemplate<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template: {err}"),
            )
                .into_response(),
        }
    }
}
