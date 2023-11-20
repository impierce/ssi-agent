use agent_store::state::ApplicationState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use cqrs_es::{persist::ViewRepository, Aggregate, View};

// TODO: These handlers should not be part of this crate (also remove axum dependency).
pub async fn query_handler<A: Aggregate, V: View<A>>(
    Path(credential_id): Path<String>,
    State(state): State<ApplicationState<A, V>>,
) -> Response {
    let view = match state.credential_query.load(&credential_id).await {
        Ok(view) => view,
        Err(err) => {
            println!("Error: {:#?}\n", err);
            return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
        }
    };
    match view {
        None => StatusCode::NOT_FOUND.into_response(),
        Some(account_view) => (StatusCode::OK, Json(account_view)).into_response(),
    }
}

// Serves as our command endpoint to make changes in a `BankAccount` aggregate.
pub async fn command_handler<A: Aggregate, V: View<A>>(
    Path(credential_id): Path<String>,
    State(state): State<ApplicationState<A, V>>,
    Json(command): Json<A::Command>,
) -> Response {
    match state
        .cqrs
        .execute_with_metadata(&credential_id, command, Default::default())
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => {
            println!("Error: {:#?}\n", err);
            (StatusCode::BAD_REQUEST, err.to_string()).into_response()
        }
    }
}
