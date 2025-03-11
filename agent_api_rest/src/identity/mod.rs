pub mod connections;
pub mod services;
pub mod well_known;

pub mod error;

use agent_identity::state::IdentityState;
use axum::{
    body::{Body, Bytes},
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use connections::{get_connection, get_connections, post_connections};
use http_body_util::BodyExt;
use hyper::StatusCode;
use services::{linked_vp::linked_vp, service, services};
use well_known::{did::did, did_configuration::did_configuration};

use crate::API_VERSION;

pub fn router(identity_state: IdentityState) -> Router {
    Router::new()
        .nest(
            API_VERSION,
            Router::new()
                .route("/connections", get(get_connections).post(post_connections))
                .route("/connections/{connection_id}", get(get_connection))
                .route("/services", get(services))
                .route("/services/{service_id}", get(service))
                .route("/services/linked-vp", post(linked_vp)),
        )
        .route("/.well-known/did.json", get(did))
        .route("/.well-known/did-configuration.json", get(did_configuration))
        .layer(middleware::from_fn(print_request_body))
        .with_state(identity_state)
}

// middleware that shows how to consume the request body upfront
async fn print_request_body(request: Request, next: Next) -> Result<impl IntoResponse, Response> {
    let request = buffer_request_body(request).await?;

    Ok(next.run(request).await)
}

// the trick is to take the request apart, buffer the body, do what you need to do, then put
// the request back together
async fn buffer_request_body(request: Request) -> Result<Request, Response> {
    let (parts, body) = request.into_parts();

    // this won't work if the body is an long running stream
    let bytes = body
        .collect()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?
        .to_bytes();

    do_thing_with_request_body(bytes.clone());

    Ok(Request::from_parts(parts, Body::from(bytes)))
}

fn do_thing_with_request_body(bytes: Bytes) {
    tracing::info!(body = ?bytes);
}
