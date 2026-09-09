use agent_shared::config::config;
use axum::{
    extract::Query,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use tracing::debug;
use utoipa::OpenApi;

/// Controls whether the response includes all configuration values or only values explicitly provisioned at startup.
#[derive(Deserialize, utoipa::IntoParams)]
pub struct ConfigurationQuery {
    pub provisioned: Option<bool>,
}

/// Get application configuration
///
/// Returns the current application configuration. Set `provisioned=true` to return only values
/// explicitly configured at startup; defaults are omitted from that response.
// TODO: Replace the generic object schema with a public schema for the application configuration.
#[utoipa::path(
    get,
    path = "/configuration",
    operation_id = "get_application_configuration",
    tags = ["Configuration"],
    params(ConfigurationQuery),
    responses(
        (status = 200, description = "Application configuration", body = Object),
    )
)]
pub async fn configuration(Query(ConfigurationQuery { provisioned }): Query<ConfigurationQuery>) -> Response {
    debug!(provisioned, "Configuration query");

    if provisioned.unwrap_or(false) {
        Json(config().get_provisioned_config()).into_response()
    } else {
        Json(serde_json::json!(config().clone())).into_response()
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(configuration),
    tags(
        (name = "Configuration", description = "Inspect UniCore application configuration."),
    )
)]
pub struct ConfigurationApi;
