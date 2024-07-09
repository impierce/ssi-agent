use axum::Json;
use serde_json::{json, Value};

#[axum_macros::debug_handler]
pub(crate) async fn health() -> Json<Value> {
    Json(json!({
        "status": "UP"
    }))
}
