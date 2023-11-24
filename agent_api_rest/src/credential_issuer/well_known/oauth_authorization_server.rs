use agent_issuance::{
    handlers::query_handler, model::aggregate::IssuanceData, queries::IssuanceDataView, state::ApplicationState,
};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::AGGREGATE_ID;

#[axum_macros::debug_handler]
pub(crate) async fn oauth_authorization_server(
    State(state): State<ApplicationState<IssuanceData, IssuanceDataView>>,
) -> impl IntoResponse {
    match query_handler(AGGREGATE_ID.to_string(), &state).await {
        Ok(Some(view)) if view.oid4vci_data.authorization_server_metadata.is_some() => {
            (StatusCode::OK, Json(view.oid4vci_data.authorization_server_metadata)).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}
