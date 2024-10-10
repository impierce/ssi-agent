use agent_holder::state::HolderState;
use agent_shared::handlers::query_handler;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use hyper::StatusCode;
use serde_json::json;

#[axum_macros::debug_handler]
pub(crate) async fn credentials(State(state): State<HolderState>) -> Response {
    match query_handler("all_holder_credentials", &state.query.all_holder_credentials).await {
        Ok(Some(all_credentials_view)) => {
            let all_credentials = all_credentials_view
                .credentials
                .into_iter()
                .map(|(_, credential_view)| credential_view)
                .collect::<Vec<_>>();

            (StatusCode::OK, Json(all_credentials)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[axum_macros::debug_handler]
pub(crate) async fn credential(State(state): State<HolderState>, Path(holder_credential_id): Path<String>) -> Response {
    match query_handler(&holder_credential_id, &state.query.holder_credential).await {
        Ok(Some(holder_credential_view)) => (StatusCode::OK, Json(holder_credential_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
