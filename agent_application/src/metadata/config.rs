use agent_shared::config::config;
use axum::{extract::Query, response::IntoResponse, Json};
use serde::Deserialize;
use tracing::debug;

/// The query parameter `provisioned` can be used to filter the returned config values for only the ones that were explicitly provided.
/// Default configuration values are not returned, which is useful to detect which configuration values can be changed during runtime (e.g. when building a UI).
#[derive(Deserialize, Debug)]
pub struct QueryParams {
    provisioned: Option<bool>,
}

pub async fn configuration(params: Query<QueryParams>) -> impl IntoResponse {
    debug!("Query params: {:?}", params);

    if params.provisioned.unwrap_or(false) {
        // When "provisioned" query parameter is present and set to "true", then only the values are returned that have been _actively_ configured.
        // This helps implementers to understand which values can be changed during runtime and which ones are immutable.
        Json(config().get_provisioned_config())
    } else {
        Json(serde_json::json!(config().clone()))
    }
}
