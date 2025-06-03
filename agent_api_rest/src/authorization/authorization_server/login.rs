use crate::authorization::authorization_server::authorize::TEMP_MAP;
use crate::authorization::authorization_server::templates::{HtmlTemplate, LoginPageTemplate};
use crate::handlers::{command_handler, query_handler};
use crate::issuance::error::{internal_server_error, PublicError};
use agent_authorization::application::pushed_authorization_service::{
    PushedAuthorizationRequest, PushedAuthorizationService,
};
use agent_authorization::state::AuthorizationState;
use agent_issuance::{offer::command::OfferCommand, state::IssuanceState};
use axum::extract::Query;
use axum::response::Redirect;
use axum::Form;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct GetLoginQuery {
    pub client_id: String, // Client ID of the OAuth2 client
    pub request_uri: String,
}

#[axum_macros::debug_handler]
pub(crate) async fn get_login(
    State(state): State<AuthorizationState>,
    Query(GetLoginQuery { client_id, request_uri }): Query<GetLoginQuery>,
) -> Result<Response, PublicError> {
    Ok(HtmlTemplate(LoginPageTemplate {
        title: "Login to Authorization Server".to_string(),
        client_id,
        request_uri,
        error_message: None,
        success_message: None,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    client_id: String,   // Client ID of the OAuth2 client
    request_uri: String, // To restore original OAuth context after login
}

// This handles processing the login form submission (POST request)
pub async fn post_login(
    State(state): State<AuthorizationState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, PublicError> {
    let _ = *TEMP_MAP
        .lock()
        .map_err(|_| PublicError::from(internal_server_error()))?
        .entry(form.request_uri.clone())
        .or_insert(true);

    Ok(Redirect::to(&format!(
        "/auth/authorize?client_id={}&request_uri={}",
        form.client_id, form.request_uri
    ))
    .into_response())
}
