use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::query_handler;
use agent_identity::{
    document::aggregate::Document,
    state::{select_signing_key, IdentityState},
};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use identity_kit::did::DidUrl;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;
use tracing::debug;

#[derive(Deserialize, Serialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct GetDocumentsEndpoint {
    #[serde(default)]
    pub did_method: Option<SupportedDidMethod>,
}

/// List DID documents
///
/// Retrieves the list of DID documents in use by your organisation.
#[utoipa::path(
    get,
    path = "/documents",
    operation_id = "get_all_documents",
    tags = ["Identity"],
    params(GetDocumentsEndpoint),
    responses(
        (status = 200, description = "Documents retrieved successfully", body = [Document]),
        (status = 400, description = "Invalid query parameter"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_documents(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Query(GetDocumentsEndpoint { did_method }): Query<GetDocumentsEndpoint>,
) -> Result<Response, ApiError> {
    debug!("Request Params - did_method: {did_method:?}");

    let filtered_documents = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_documents",
        None,
        &state.query.all_documents,
    )
    .await?
    .map(|all_documents_view| {
        let filtered_documents: Vec<_> = crate::utils::newest_first(all_documents_view.documents)
            .filter(|document| {
                did_method
                    .as_ref()
                    .is_none_or(|method| document.did_method.as_ref() == Some(method))
            })
            .collect();

        filtered_documents
    })
    .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_documents)).into_response())
}

/// Get DID document by ID
///
/// Retrieves a DID document of your organisation by its ID.
#[utoipa::path(
    get,
    path = "/documents/{document_id}",
    operation_id = "get_document_by_id",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Document retrieved successfully", body = Document),
        (status = 400, description = "Invalid path parameter"),
        (status = 404, description = "Document not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_document(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Path(document_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &document_id,
        Some(&document_id),
        &state.query.document,
    )
    .await?
    .map(|document_view| (StatusCode::OK, Json(document_view)).into_response())
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectSigningKeyRequest {
    /// The verification method to sign with, in one of your organisation's DID documents.
    #[schema(value_type = String, example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: DidUrl,
}

/// Select signing key
///
/// Signs with the given verification method from now on, for the algorithm of its key, in the DID document it belongs
/// to. Selecting the key that is already selected is a no-op.
#[utoipa::path(
    post,
    path = "/select-signing-key",
    operation_id = "select_signing_key",
    tags = ["Identity"],
    request_body(content = inline(SelectSigningKeyRequest), content_type = "application/json"),
    responses(
        (status = 204, description = "Signing key selected"),
        (status = 400, description = "The verification method's key names no supported algorithm"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 404, description = "No DID document has the verification method"),
        (status = 409, description = "The verification method's key is not available to sign with"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_select_signing_key(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(SelectSigningKeyRequest { verification_method_id }): Json<SelectSigningKeyRequest>,
) -> Result<StatusCode, ApiError> {
    select_signing_key(
        &state,
        actor.map_or(Caller::Anonymous, Caller::Actor),
        verification_method_id,
    )
    .await
    .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::services::IdentityServices;
    use agent_kms::state::kms_state;
    use axum::{body::Body, extract::Request};
    use shared_kernel::authorization::NoActorExtractor;
    use shared_kernel::EventBusHandle;
    use tower::ServiceExt;

    async fn app(kms_enabled: bool) -> axum::Router {
        let event_bus = EventBusHandle::default();
        let identity_state = agent_identity::identity_state(
            &agent_store::in_memory::InMemory,
            IdentityServices::default(),
            &event_bus,
        )
        .await;
        let kms_state = if kms_enabled {
            Some(Arc::new(
                kms_state(&agent_store::in_memory::InMemory, &event_bus, true, Default::default()).await,
            ))
        } else {
            None
        };

        crate::app(
            crate::ApiState {
                identity_state: Some(Arc::new(identity_state)),
                kms_state,
                ..Default::default()
            },
            Arc::new(NoActorExtractor),
        )
    }

    async fn select_signing_key(app: axum::Router) -> (StatusCode, Vec<u8>) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v0/select-signing-key")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"verificationMethodId":"did:web:example.org#key"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();

        (
            status,
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
    }

    #[tokio::test]
    async fn selecting_a_signing_key_does_not_exist_without_the_key_management_system() {
        let (status, body) = select_signing_key(app(false).await).await;

        assert_eq!((status, body.is_empty()), (StatusCode::NOT_FOUND, true));
    }

    #[tokio::test]
    async fn selecting_a_signing_key_of_an_unknown_did_answers_not_found() {
        let (_, body) = select_signing_key(app(true).await).await;

        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            problem["type"],
            crate::error::type_url("identity#verification-method-not-found")
        );
    }
}
