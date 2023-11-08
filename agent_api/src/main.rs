use agent_core::{user::Basic as BasicAuth, user::User};
use axum::{
    extract::{Path, TypedHeader},
    headers::{authorization::Basic, Authorization},
    http::{
        header::{self, CONTENT_TYPE},
        HeaderValue, Response, StatusCode,
    },
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::{
    cors::CorsLayer,
    trace::{self, DefaultMakeSpan, DefaultOnRequest, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    agent_core::init().await.unwrap();

    let app = Router::new()
        .route("/.well-known/openid-credential-issuer", get(well_known_issuer))
        .route(
            "/.well-known/oauth-authorization-server",
            get(well_known_authorization_server),
        )
        .route("/token", post(token))
        .route("/credentials", post(create_credential))
        .route("/credentials/:id", get(get_credential))
        .route("/credentials/:id/sign", post(sign_credential))
        .route("/offers", post(create_offer))
        .route("/events", get(get_all_events))
        .layer(
            tower::ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                        .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
                )
                .layer(
                    CorsLayer::new()
                        .allow_origin("http://localhost:5175".parse::<HeaderValue>().unwrap())
                        .allow_headers([CONTENT_TYPE]),
                ),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], 3033));

    axum::Server::bind(&addr).serve(app.into_make_service()).await.unwrap();
}

#[axum_macros::debug_handler]
async fn create_credential(
    basic_auth: Option<TypedHeader<Authorization<Basic>>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    // TODO: also check against configured basic auth credentials
    if basic_auth.is_none() {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let (id, credential) = agent_core::create_credential(None, payload).await.unwrap();
    (
        StatusCode::CREATED,
        [
            (header::LOCATION, format!("/credential/{}", id)),
            (header::CONTENT_TYPE, "application/json".to_string()),
        ],
        serde_json::to_string_pretty(&credential).unwrap(),
    )
        .into_response()
}

#[axum_macros::debug_handler]
async fn get_credential(Path(id): Path<String>) -> Json<Value> {
    let credential = agent_core::get_credential(id).await.unwrap();
    Json(serde_json::to_value(credential).unwrap())
}

#[axum_macros::debug_handler]
async fn sign_credential(Path(id): Path<String>) -> Json<Value> {
    let credential = agent_core::sign_credential(id).await.unwrap();
    Json(serde_json::to_value(credential).unwrap())
}

#[axum_macros::debug_handler]
async fn create_offer(Json(payload): Json<CredentialOfferRequest>) -> impl IntoResponse {
    let offer = agent_core::create_credential_offer(payload.credential_ids).await;
    (
        StatusCode::CREATED,
        // [(header::CONTENT_TYPE, "application/json".to_string())],
        // serde_json::to_string_pretty(&offer.unwrap()).unwrap(),
        offer.unwrap(),
    )
        .into_response()
}

/// https://identity.foundation/.well-known/resources/did-configuration/
#[axum_macros::debug_handler]
async fn well_known_issuer() -> Json<Value> {
    Json(json!({
        "credential_issuer": "did:key:1234567890",
        "credential_endpoint": "http://localhost:3033/v1/openid4vci/credential",
        "batch_credential_endpoint": "http://localhost:3033/v1/openid4vci/batch_credential",
        "credentials_supported": [
            {
                "format": "jwt_vc_json",
                "id": "UniversityDegree_JWT",
                "types": [
                    "VerifiableCredential",
                    "UniversityDegreeCredential"
                ],
            }
        ]
    }))
}

#[axum_macros::debug_handler]
async fn well_known_authorization_server() -> Json<Value> {
    Json(json!({
        "issuer": "http://daniels-macbook-pro.local:3033",
        "authorization_endpoint": "http://daniels-macbook-pro.local:3033/authorize",
        "token_endpoint": "http://daniels-macbook-pro.local:3033/token"
    }))
}

#[axum_macros::debug_handler]
async fn token() {
    // TODO: do something here ...
}

#[axum_macros::debug_handler]
async fn get_all_events() -> Json<Value> {
    let events = agent_core::get_all_credential_events().await.unwrap();
    Json(events)
}

#[derive(Deserialize)]
struct CredentialOfferRequest {
    credential_ids: Vec<uuid::Uuid>,
}
