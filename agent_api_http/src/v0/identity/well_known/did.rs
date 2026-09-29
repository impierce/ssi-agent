use agent_identity::{
    document::{aggregate::Status, openapi::DidDocument},
    state::IdentityState,
};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

use crate::handlers::public_query_handler;
use crate::v0::openapi::PROTOCOL_TAG;

/// Get the `did:web` DID document
///
/// Returns the enabled `did:web` DID document of this application, as resolved by
/// [did:web](https://w3c-ccg.github.io/did-method-web/) resolvers.
#[utoipa::path(
    get,
    path = "/.well-known/did.json",
    operation_id = "well_known_did_json",
    tags = ["DID", PROTOCOL_TAG],
    responses(
        (status = 200, description = "DID document", body = DidDocument),
        (status = 404, description = "No enabled `did:web` DID document exists"),
        (status = 500, description = "The DID document could not be retrieved"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn did(State(state): State<Arc<IdentityState>>) -> Result<Response, ApiError> {
    public_query_handler("all_documents", &state.query.all_documents)
        .await?
        .and_then(|all_documents_view| {
            all_documents_view.documents.into_values().find_map(|document| {
                document.document.and_then(|core_document| {
                    (document.status != Status::Disabled && document.did_method == Some(SupportedDidMethod::Web))
                        .then_some((StatusCode::OK, Json(core_document)).into_response())
                })
            })
        })
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
