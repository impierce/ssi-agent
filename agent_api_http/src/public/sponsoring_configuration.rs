use agent_shared::config::config;
use axum::{
    response::{IntoResponse as _, Response},
    Json,
};
use http::StatusCode;
use serde::Serialize;
use serde_with::skip_serializing_none;
use url::Url;

#[skip_serializing_none]
#[derive(Serialize)]
pub struct SponsoringConfiguration {
    pub name: String,
    #[serde(default)]
    pub logo_uri: Option<Url>,
    pub iota_address: String,
}

#[axum_macros::debug_handler]
pub async fn sponsoring_configuration() -> Result<Response, StatusCode> {
    let configuration = config().clone();

    let display = configuration
        .display
        .clone()
        .pop()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let iota_address = configuration.iota_address.ok_or(StatusCode::NOT_FOUND)?;

    Ok((
        StatusCode::OK,
        Json(SponsoringConfiguration {
            name: display.name,
            logo_uri: display.logo.and_then(|logo| logo.uri),
            iota_address,
        }),
    )
        .into_response())
}
