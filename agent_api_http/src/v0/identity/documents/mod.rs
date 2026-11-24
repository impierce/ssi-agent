use crate::error::IntoApiErrorExt;
use crate::extractors::{JsonBody, PathParams, QueryParams, RequestActor};
use crate::handlers::{caller, query_handler};
use agent_identity::{
    document::{aggregate::Document, key_source::KeySource, keys},
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
use identity_kit::did::DidUrl;
use jsonwebtoken::Algorithm;
use serde::{Deserialize, Serialize};
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
pub struct AddKeyToDocumentRequest {
    /// The ID of a key created in the Key Management System.
    #[schema(example = "0b6a7f8e-3f4c-4d2a-9b1e-5c8d7e6f5a4b")]
    pub key_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyToDocumentResponse {
    /// The verification method of the key, named by the thumbprint of its public key.
    #[schema(value_type = String, example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: DidUrl,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectSigningKeyRequest {
    /// The verification method to sign with, in one of your organisation's DID documents.
    #[schema(value_type = String, example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: DidUrl,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeselectSigningKeyRequest {
    /// The algorithm whose configured key signs again.
    #[schema(value_type = String, example = "ES256")]
    pub algorithm: Algorithm,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveKeyFromDocumentRequest {
    /// The verification method of an added key.
    #[schema(value_type = String, example = "did:web:example.org#kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")]
    pub verification_method_id: DidUrl,
}

/// The key an algorithm of your organisation's `did:web` document signs with, as UniCore last saw it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = SigningKey)]
pub struct SigningKeyDto {
    #[schema(value_type = String, example = "ES256")]
    pub algorithm: Algorithm,
    /// The verification method that signs now.
    #[schema(value_type = String)]
    pub verification_method_id: DidUrl,
    /// The verification method selected to sign. When it differs from `verificationMethodId`, its key isn't available
    /// and the configured key signs instead.
    #[schema(value_type = Option<String>)]
    pub selected_verification_method_id: Option<DidUrl>,
    /// Why the selected key, or else the signing key, can't sign: it couldn't be loaded, it failed the last time it
    /// signed, or it was deleted. `null` doesn't prove the key works: a key deleted or disabled in its provider shows
    /// `null` until a signature with it fails. Selecting another key with `select-signing-key`, or the configured one
    /// with `deselect-signing-key`, resolves it.
    pub problem: Option<String>,
}

impl From<keys::SigningKey> for SigningKeyDto {
    fn from(signing_key: keys::SigningKey) -> Self {
        Self {
            algorithm: signing_key.algorithm,
            verification_method_id: signing_key.verification_method_id,
            selected_verification_method_id: signing_key.selected_verification_method_id,
            problem: signing_key.problem,
        }
    }
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
        (status = 501, description = "The key exists, but this instance can't sign with it"),
        (status = 502, description = "The key can't be had now, such as when its key provider doesn't answer"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_add_key_to_document(
    State((state, key_source)): State<(Arc<IdentityState>, Arc<dyn KeySource>)>,
    RequestActor(actor): RequestActor,
    JsonBody(AddKeyToDocumentRequest { key_id }): JsonBody<AddKeyToDocumentRequest>,
) -> Result<Response, ApiError> {
    let verification_method_id = keys::add_key_to_document(&state, key_source.as_ref(), caller(actor), &key_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok((
        StatusCode::OK,
        Json(AddKeyToDocumentResponse { verification_method_id }),
    )
        .into_response())
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
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_remove_key_from_document(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    JsonBody(RemoveKeyFromDocumentRequest { verification_method_id }): JsonBody<RemoveKeyFromDocumentRequest>,
) -> Result<Response, ApiError> {
    keys::remove_key_from_document(&state, caller(actor), &verification_method_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
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
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_select_signing_key(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    JsonBody(SelectSigningKeyRequest { verification_method_id }): JsonBody<SelectSigningKeyRequest>,
) -> Result<Response, ApiError> {
    keys::select_signing_key(&state, caller(actor), &verification_method_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
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
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn post_deselect_signing_key(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    JsonBody(DeselectSigningKeyRequest { algorithm }): JsonBody<DeselectSigningKeyRequest>,
) -> Result<Response, ApiError> {
    keys::deselect_signing_key(&state, caller(actor), algorithm)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Get signing keys
///
/// Lists, per algorithm, the key your organisation's `did:web` document signs with now, the key selected to sign, and
/// why that key can't sign, if it can't.
///
/// Keys aren't checked in their key provider until they sign: a key deleted or disabled there is still listed as
/// signing, without a `problem`, until the next signature with it fails. From then on, its `problem` says why, and the
/// configured key signs instead while UniCore retries the key in the background.
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
) -> Result<Response, ApiError> {
    let signing_keys: Vec<SigningKeyDto> = keys::get_signing_keys(&state, caller(actor))
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .into_iter()
        .map(SigningKeyDto::from)
        .collect();

    Ok((StatusCode::OK, Json(signing_keys)).into_response())
}

#[cfg(test)]
mod tests {
    use crate::{app_with_base_path, ApiState};
    use agent_identity::{
        document::key_source::{KeySource, NoKeys},
        identity_state,
        services::IdentityServices,
        state::IdentityState,
    };
    use agent_store::in_memory::InMemory;
    use async_trait::async_trait;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use shared_kernel::authorization::{
        AuthorizationChecker, AuthorizationError, AuthorizationRequest, NoActorExtractor,
    };
    use shared_kernel::event_bus::EventBusHandle;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct DenyAll;

    #[async_trait]
    impl AuthorizationChecker for DenyAll {
        async fn is_authorized(&self, _request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            Err(AuthorizationError::Forbidden)
        }
    }

    /// An identity without documents.
    async fn identity() -> IdentityState {
        identity_state(&InMemory, IdentityServices::default(), &EventBusHandle::default()).await
    }

    /// The app with `identity`, and with a key source, as with the Key Management System, when `kms` is set.
    async fn app(identity: IdentityState, kms: bool) -> Router {
        app_with_base_path(
            ApiState {
                identity_state: Some(Arc::new(identity)),
                key_source: kms.then(|| Arc::new(NoKeys) as Arc<dyn KeySource>),
                ..Default::default()
            },
            Arc::new(NoActorExtractor),
            "/",
        )
    }

    async fn send(app: &Router, method: &str, uri: &str, body: Option<Value>) -> StatusCode {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        response.into_body().collect().await.unwrap();
        status
    }

    #[tokio::test]
    async fn adding_a_key_to_a_document_does_not_exist_without_the_key_management_system() {
        let app = app(identity().await, false).await;

        let status = send(
            &app,
            "POST",
            "/v0/add-key-to-document",
            Some(json!({ "keyId": "key-id" })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn selecting_a_signing_key_does_not_exist_without_the_key_management_system() {
        let app = app(identity().await, false).await;

        let body = json!({ "verificationMethodId": "did:web:example.org#key" });
        let status = send(&app, "POST", "/v0/select-signing-key", Some(body)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_document_key_routes_do_not_exist_without_the_key_management_system() {
        let app = app(identity().await, false).await;

        for (method, uri) in [
            ("POST", "/v0/remove-key-from-document"),
            ("POST", "/v0/deselect-signing-key"),
            ("GET", "/v0/get-signing-keys"),
        ] {
            assert_eq!(send(&app, method, uri, None).await, StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn adding_a_key_without_a_did_web_document_answers_conflict() {
        let app = app(identity().await, true).await;

        let status = send(
            &app,
            "POST",
            "/v0/add-key-to-document",
            Some(json!({ "keyId": "key-id" })),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn removing_a_key_without_a_did_web_document_answers_conflict() {
        let app = app(identity().await, true).await;

        let body = json!({ "verificationMethodId": "did:web:example.org#key" });
        let status = send(&app, "POST", "/v0/remove-key-from-document", Some(body)).await;

        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn selecting_a_signing_key_of_an_unknown_did_answers_not_found() {
        let app = app(identity().await, true).await;

        let body = json!({ "verificationMethodId": "did:web:example.org#key" });
        let status = send(&app, "POST", "/v0/select-signing-key", Some(body)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn lists_no_signing_keys_without_a_did_web_document() {
        let app = app(identity().await, true).await;
        let request = Request::get("/v0/get-signing-keys").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();

        let status = response.status();
        let body: Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!((status, body), (StatusCode::OK, json!([])));
    }

    #[tokio::test]
    async fn listing_signing_keys_unauthorized_answers_forbidden() {
        let mut identity = identity().await;
        identity.authorization_checker = Arc::new(DenyAll);
        let app = app(identity, true).await;

        let status = send(&app, "GET", "/v0/get-signing-keys", None).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
