use agent_shared::config::config;
use agent_shared::config::EventStoreType;
use agent_store::postgres::check_connection;
use axum::http::StatusCode;
use axum::response::IntoResponse;

#[axum_macros::debug_handler]
pub async fn readyz_handler() -> impl IntoResponse {
    // check database connection
    // check message queue connection

    let event_store_type = config().event_store.type_.clone();

    // write code: if config is postgres, then call the postgres check_connection function
    let status_code = match event_store_type {
        EventStoreType::InMemory => {
            println!("Checking Postgres connection ...");

            // check_connection().await;

            if check_connection().await {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
            // StatusCode::OK

            // if postgres::check_connection().await {
            //     return axum::http::StatusCode::OK;
            // } else {
            //     return axum::http::StatusCode::SERVICE_UNAVAILABLE;
            // }
            // if let Err(e) = postgres::check_connection().await {
            //     return axum::http::StatusCode::SERVICE_UNAVAILABLE;
            // }
        }
        EventStoreType::Postgres => {
            // do nothing
            StatusCode::OK
        }
    };

    // Response::new("foobar".into())

    status_code
}
