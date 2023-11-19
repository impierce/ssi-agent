use agent_issuance::state::new_application_state;
use agent_store::state::ApplicationState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use cqrs_es::{persist::ViewRepository, Aggregate, View};

#[tokio::main]
async fn main() {
    let state = new_application_state().await;
    // Configure the Axum routes and services.
    // For this example a single logical endpoint is used and the HTTP method
    // distinguishes whether the call is a command or a query.
    let router = Router::new()
        .route(
            "/credential/:credential_id",
            get(query_handler).post(command_handler),
        )
        // .route("/actual_credential/:credential_id", post(command_handler))
        .with_state(state);
    // Start the Axum server.
    axum::Server::bind(&"0.0.0.0:3030".parse().unwrap())
        .serve(router.into_make_service())
        .await
        .unwrap();
}

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
