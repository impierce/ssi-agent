use agent_shared::config::{config, load_raw_config};
use axum::{extract::Query, response::IntoResponse, Json};
use serde::Deserialize;
use tracing::debug;

/// The query parameter `provisioned` can be used to filter the returned config values for only the ones that were explicitly provided.
/// Default configuration values are not returned, which is useful to detect which configuration values can be changed during runtime (e.g. when building a UI).
#[derive(Deserialize, Debug)]
pub struct QueryParams {
    provisioned: Option<bool>,
}

// #[axum_macros::debug_handler]
pub async fn app_config(params: Query<QueryParams>) -> impl IntoResponse {
    debug!("Query params: {:?}", params);

    if params.provisioned.unwrap_or(false) {
        // If "provisioned", then show if the value was provisioned (true/false) for each field in the returned config.
        // This helps implementers to understand which values they can change and which ones are immutable.
        let raw = load_raw_config().unwrap();
        let v: serde_json::Value = raw.try_deserialize().unwrap();
        Json(v)
    } else {
        Json(serde_json::to_value(config().clone()).unwrap())
    }
}
