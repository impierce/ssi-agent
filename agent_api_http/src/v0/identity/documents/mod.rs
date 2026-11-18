use crate::error::IntoApiErrorExt;
use crate::extractors::{PathParams, QueryParams, RequestActor};
use crate::handlers::query_handler;
use agent_identity::{
    document::aggregate::Document,
    state::{
        add_key, deselect_signing_key, get_signing_keys, remove_key_from_document, select_signing_key, IdentityState,
        SigningKeyStatus,
    },
};
use agent_shared::config::SupportedDidMethod;
use axum::{
    extract::State,
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
#[into_params(parameter_in = Query)]
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
    QueryParams(GetDocumentsEndpoint { did_method }): QueryParams<GetDocumentsEndpoint>,
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
    params(("document_id" = String, Path, description = "Document ID")),
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
    PathParams(document_id): PathParams<String>,
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyToDocumentRequest {
    /// The ID of a key created in the Key Management System.
    #[schema(example = "0b6a7f8e-3f4c-4d2a-9b1e-5c8d7e6f5a4b")]
    pub key_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyToDocumentResponse {
    /// The verification method of the key, named by the thumbprint of its public key.
    #[schema(example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: String,
}

/// Add key to document
///
/// Adds the public key of a key created in the Key Management System to your organisation's `did:web` document, as a
/// verification method. The key is available to sign with right away, but only signs once it is selected as the
/// signing key. Adding a key again changes nothing.
#[utoipa::path(
    post,
    path = "/add-key-to-document",
    operation_id = "add_key_to_document",
    tags = ["Identity"],
    request_body(content = inline(AddKeyToDocumentRequest), content_type = "application/json"),
    responses(
        (status = 200, description = "Key added", body = AddKeyToDocumentResponse),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 404, description = "No key has the given ID"),
        (status = 409, description = "There is no enabled `did:web` document to add the key to"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_add_key_to_document(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(AddKeyToDocumentRequest { key_id }): Json<AddKeyToDocumentRequest>,
) -> Result<Json<AddKeyToDocumentResponse>, ApiError> {
    let verification_method_id = add_key(&state, actor.map_or(Caller::Anonymous, Caller::Actor), &key_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(Json(AddKeyToDocumentResponse {
        verification_method_id: verification_method_id.to_string(),
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeselectSigningKeyRequest {
    /// The algorithm whose configured key signs again.
    #[schema(value_type = String, example = "ES256")]
    pub algorithm: jsonwebtoken::Algorithm,
}

/// Deselect signing key
///
/// Signs with the configured key of the given algorithm again in your organisation's `did:web` document. Deselecting
/// an algorithm without a selected key is a no-op.
#[utoipa::path(
    post,
    path = "/deselect-signing-key",
    operation_id = "deselect_signing_key",
    tags = ["Identity"],
    request_body(content = inline(DeselectSigningKeyRequest), content_type = "application/json"),
    responses(
        (status = 204, description = "Signing key deselected"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 409, description = "There is no enabled `did:web` document"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_deselect_signing_key(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(DeselectSigningKeyRequest { algorithm }): Json<DeselectSigningKeyRequest>,
) -> Result<StatusCode, ApiError> {
    deselect_signing_key(&state, actor.map_or(Caller::Anonymous, Caller::Actor), algorithm)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveKeyFromDocumentRequest {
    /// The verification method of an added key.
    #[schema(value_type = String, example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: DidUrl,
}

/// Remove key from document
///
/// Removes the verification method of a key added with `add-key-to-document` from your organisation's `did:web`
/// document. Credentials it signed no longer verify. The selected signing key of an algorithm can't be removed:
/// select another key or deselect it first.
#[utoipa::path(
    post,
    path = "/remove-key-from-document",
    operation_id = "remove_key_from_document",
    tags = ["Identity"],
    request_body(content = inline(RemoveKeyFromDocumentRequest), content_type = "application/json"),
    responses(
        (status = 204, description = "Key removed"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
        (status = 409, description = "The key is the selected signing key, not an added key, or there is no enabled `did:web` document"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_remove_key_from_document(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(RemoveKeyFromDocumentRequest { verification_method_id }): Json<RemoveKeyFromDocumentRequest>,
) -> Result<StatusCode, ApiError> {
    remove_key_from_document(
        &state,
        actor.map_or(Caller::Anonymous, Caller::Actor),
        verification_method_id,
    )
    .await
    .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// The key an algorithm of your organisation's `did:web` document signs with.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = SigningKey)]
pub struct SigningKeyDto {
    #[schema(value_type = String, example = "ES256")]
    pub algorithm: jsonwebtoken::Algorithm,
    /// The verification method that signs now.
    #[schema(value_type = String)]
    pub verification_method_id: DidUrl,
    /// The verification method selected to sign. When it differs from `verificationMethodId`, its key isn't available
    /// and the configured key signs instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub selected_verification_method_id: Option<DidUrl>,
    /// Why the selected key, or else the signing key, can't sign: it isn't available, or it failed the last time it
    /// signed. Selecting another key with `select-signing-key`, or the configured one with `deselect-signing-key`,
    /// resolves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Get signing keys
///
/// Lists, per algorithm, the key your organisation's `did:web` document signs with now, the key selected to sign, and
/// why that key can't sign, if it can't.
#[utoipa::path(
    get,
    path = "/get-signing-keys",
    operation_id = "get_signing_keys",
    tags = ["Identity"],
    responses(
        (status = 200, description = "The signing keys", body = [SigningKeyDto]),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_signing_keys_endpoint(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Json<Vec<SigningKeyDto>>, ApiError> {
    let statuses = get_signing_keys(&state, actor.map_or(Caller::Anonymous, Caller::Actor))
        .await
        .map_err(|error| {
            tracing::error!("Listing the signing keys failed: {error:#}");
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    Ok(Json(
        statuses
            .into_iter()
            .map(
                |SigningKeyStatus {
                     algorithm,
                     verification_method_id,
                     selected_verification_method_id,
                     problem,
                 }| SigningKeyDto {
                    algorithm,
                    verification_method_id,
                    selected_verification_method_id,
                    problem,
                },
            )
            .collect(),
    ))
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
        post(
            app,
            "/v0/select-signing-key",
            r#"{"verificationMethodId":"did:web:example.org#key"}"#,
        )
        .await
    }

    async fn add_key_to_document(app: axum::Router) -> (StatusCode, Vec<u8>) {
        post(app, "/v0/add-key-to-document", r#"{"keyId":"key"}"#).await
    }

    async fn post(app: axum::Router, uri: &str, body: &'static str) -> (StatusCode, Vec<u8>) {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
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

    #[tokio::test]
    async fn adding_a_key_to_a_document_does_not_exist_without_the_key_management_system() {
        let (status, body) = add_key_to_document(app(false).await).await;

        assert_eq!((status, body.is_empty()), (StatusCode::NOT_FOUND, true));
    }

    async fn get_signing_keys(app: axum::Router) -> (StatusCode, Vec<u8>) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v0/get-signing-keys")
                    .body(Body::empty())
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
    async fn the_document_key_routes_do_not_exist_without_the_key_management_system() {
        let responses = [
            get_signing_keys(app(false).await).await,
            post(app(false).await, "/v0/deselect-signing-key", r#"{"algorithm":"ES256"}"#).await,
            post(
                app(false).await,
                "/v0/remove-key-from-document",
                r#"{"verificationMethodId":"did:web:example.org#key"}"#,
            )
            .await,
        ];

        assert!(responses
            .iter()
            .all(|(status, body)| *status == StatusCode::NOT_FOUND && body.is_empty()));
    }

    #[tokio::test]
    async fn lists_no_signing_keys_without_a_did_web_document() {
        let response = get_signing_keys(app(true).await).await;

        assert_eq!(response, (StatusCode::OK, b"[]".to_vec()));
    }

    #[tokio::test]
    async fn removing_a_key_without_a_did_web_document_answers_conflict() {
        let (_, body) = post(
            app(true).await,
            "/v0/remove-key-from-document",
            r#"{"verificationMethodId":"did:web:example.org#key"}"#,
        )
        .await;

        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            problem["type"],
            crate::error::type_url("identity#did-web-document-not-found")
        );
    }

    #[tokio::test]
    async fn adding_a_key_without_a_did_web_document_answers_conflict() {
        let (_, body) = add_key_to_document(app(true).await).await;

        let problem: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            problem["type"],
            crate::error::type_url("identity#did-web-document-not-found")
        );
    }
}
