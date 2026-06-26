use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::{command_handler, public_query_handler, query_handler};
use agent_issuance::public_offer::aggregate::PublicOffer;
use agent_issuance::public_offer::command::PublicOfferCommand;
use agent_issuance::public_offer::error::PublicOfferError;
use agent_issuance::state::IssuanceState;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::Status;
use axum::Extension;
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// public-offer aggregate ID is namespaced, so it doesn't collide with existing offer streams.
fn public_offer_aggregate_id(offer_id: &str) -> String {
    format!("public_offer:{offer_id}")
}

/// Validates that a schema only contains const-only leaf fields.
/// A const-only leaf is a field without properties that has `const` set.
fn validate_schema_has_only_consts(schema: &Option<Value>) -> Result<(), PublicOfferError> {
    match schema {
        None => Ok(()),
        Some(root) => {
            if !root.is_object() {
                return Ok(());
            }

            let properties = root
                .get("properties")
                .and_then(|p| p.as_object())
                .ok_or(PublicOfferError::TemplateNotEligible)?;

            for (_key, prop) in properties {
                validate_leaf_is_const(prop)?;
            }

            Ok(())
        }
    }
}

/// Recursively validates that a schema property is either a const value or a container with const children.
fn validate_leaf_is_const(prop: &Value) -> Result<(), PublicOfferError> {
    if !prop.is_object() {
        return Err(PublicOfferError::TemplateNotEligible);
    }

    let obj = prop.as_object().unwrap();

    // If this has nested properties, it's a container—recurse into children.
    if let Some(nested_props) = obj.get("properties").and_then(|p| p.as_object()) {
        for (_key, nested) in nested_props {
            validate_leaf_is_const(nested)?;
        }
        return Ok(());
    }

    // If this is a leaf and has const, it's valid.
    if obj.contains_key("const") {
        return Ok(());
    }

    // Leaf without const is invalid.
    Err(PublicOfferError::TemplateNotEligible)
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicOfferStatusDto {
    pub id: String,
    pub template_id: String,
    pub amount_issued: u32,
    pub status: PublicOfferStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub enum PublicOfferStatus {
    Active,
    Inactive,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePublicOfferRequest {
    pub offer_id: String,
    pub template_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TakePublicOfferOfflineRequest {
    pub offer_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TakePublicOfferOnlineRequest {
    pub offer_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletePublicOfferRequest {
    pub offer_id: String,
}

impl From<&PublicOffer> for PublicOfferStatusDto {
    fn from(offer: &PublicOffer) -> Self {
        PublicOfferStatusDto {
            id: offer.id.clone(),
            template_id: offer.template_id.clone(),
            amount_issued: 0,
            status: if offer.active && !offer.deleted {
                PublicOfferStatus::Active
            } else {
                PublicOfferStatus::Inactive
            },
        }
    }
}

/// Get all public offers
#[utoipa::path(
    get,
    path = "/get-all-public-offers",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Public offers retrieved successfully", body = [PublicOfferStatusDto])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_public_offers(
    State(issuance_state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let all_offers = query_handler(
        issuance_state.authorization_checker.clone(),
        actor.clone(),
        "all_public_offers",
        &issuance_state.query.all_public_offers,
    )
    .await?
    .unwrap_or_default();

    let mut offers = Vec::with_capacity(all_offers.offers.len());

    for public_offer in all_offers.offers.values() {
        let mut dto = PublicOfferStatusDto::from(public_offer);
        if let Some(offer_view) = query_handler(
            issuance_state.authorization_checker.clone(),
            actor.clone(),
            &public_offer.id,
            &issuance_state.query.offer,
        )
        .await?
        {
            dto.amount_issued = offer_view.successful_issuances;
        }
        offers.push(dto);
    }

    Ok((StatusCode::OK, Json(offers)).into_response())
}

/// Create a public offer mapping
#[utoipa::path(
    post,
    path = "/create-public-offer",
    tags = ["Issuance"],
    request_body = CreatePublicOfferRequest,
    responses(
        (status = 201, description = "Public offer created successfully"),
        (status = 404, description = "Template or offer not found"),
        (status = 400, description = "Template schema invalid for public offers")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_public_offer(
    State(issuance_state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Extension(library_state): Extension<Arc<LibraryState>>,
    Json(CreatePublicOfferRequest { offer_id, template_id }): Json<CreatePublicOfferRequest>,
) -> Result<Response, ApiError> {
    if query_handler(
        issuance_state.authorization_checker.clone(),
        actor.clone(),
        &offer_id,
        &issuance_state.query.offer,
    )
    .await?
    .is_none()
    {
        return Err(ApiError::new(StatusCode::NOT_FOUND));
    }

    let template = query_handler(
        library_state.authorization_checker.clone(),
        actor.clone(),
        &template_id,
        &library_state.query.template,
    )
        .await
        .map_err(|_| PublicOfferError::TemplateNotFound.into_api_error())?
        .ok_or_else(|| PublicOfferError::TemplateNotFound.into_api_error())?;

    // Only non-deleted templates can be offered publicly
    if template.status == Status::Deleted {
        return Err(PublicOfferError::TemplateNotFound.into_api_error());
    }

    // Validate that the template schema only contains const-only leaf fields
    validate_schema_has_only_consts(&template.schema).map_err(|e| e.into_api_error())?;

    let command = PublicOfferCommand::Create {
        offer_id: offer_id.clone(),
        template_id,
    };

    let aggregate_id = public_offer_aggregate_id(&offer_id);
    command_handler(
        issuance_state.authorization_checker.clone(),
        actor,
        &aggregate_id,
        &issuance_state.command.public_offer,
        command,
    )
    .await?;

    Ok((StatusCode::CREATED).into_response())
}

/// Take a public offer offline
#[utoipa::path(
    post,
    path = "/take-public-offer-offline",
    tags = ["Issuance"],
    request_body = TakePublicOfferOfflineRequest,
    responses(
        (status = 204, description = "Public offer taken offline successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_offline(
    State(issuance_state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Json(TakePublicOfferOfflineRequest { offer_id }): Json<TakePublicOfferOfflineRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::TakeOffline {
        offer_id: offer_id.clone(),
    };

    let aggregate_id = public_offer_aggregate_id(&offer_id);
    command_handler(
        issuance_state.authorization_checker.clone(),
        actor,
        &aggregate_id,
        &issuance_state.command.public_offer,
        command,
    )
    .await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Take a public offer online
#[utoipa::path(
    post,
    path = "/take-public-offer-online",
    tags = ["Issuance"],
    request_body = TakePublicOfferOnlineRequest,
    responses(
        (status = 204, description = "Public offer taken online successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn take_public_offer_online(
    State(issuance_state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Json(TakePublicOfferOnlineRequest { offer_id }): Json<TakePublicOfferOnlineRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::TakeOnline {
        offer_id: offer_id.clone(),
    };

    let aggregate_id = public_offer_aggregate_id(&offer_id);
    command_handler(
        issuance_state.authorization_checker.clone(),
        actor,
        &aggregate_id,
        &issuance_state.command.public_offer,
        command,
    )
    .await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Delete a public offer
#[utoipa::path(
    post,
    path = "/delete-public-offer",
    tags = ["Issuance"],
    request_body = DeletePublicOfferRequest,
    responses(
        (status = 204, description = "Public offer deleted successfully"),
        (status = 404, description = "Public offer not found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_public_offer(
    State(issuance_state): State<Arc<IssuanceState>>,
    RequestActor(actor): RequestActor,
    Json(DeletePublicOfferRequest { offer_id }): Json<DeletePublicOfferRequest>,
) -> Result<Response, ApiError> {
    let command = PublicOfferCommand::Delete {
        offer_id: offer_id.clone(),
    };

    let aggregate_id = public_offer_aggregate_id(&offer_id);
    command_handler(
        issuance_state.authorization_checker.clone(),
        actor,
        &aggregate_id,
        &issuance_state.command.public_offer,
        command,
    )
    .await?;

    Ok((StatusCode::NO_CONTENT).into_response())
}

/// Check if a public offer can be resolved (is active and not deleted)
pub(crate) async fn can_resolve_public_offer(state: &Arc<IssuanceState>, offer_id: &str) -> Result<bool, ApiError> {
    let aggregate_id = public_offer_aggregate_id(offer_id);

    match public_query_handler(&aggregate_id, &state.query.public_offer).await? {
        Some(offer) => Ok(offer.active && !offer.deleted),
        // If there is no public-offer record, treat it as a normal offer.
        None => Ok(true),
    }
}

#[cfg(test)]
mod tests {
    use crate::handlers::command_handler;
    use crate::tests::TEMPLATE_ID;
    use crate::v0::issuance::credentials::tests::{create_test_template_with_auth, credentials, setup_library_state};
    use crate::v0::issuance::router;
    use crate::API_VERSION;
    use agent_issuance::services::IssuanceServices;
    use agent_issuance::state::initialize;
    use agent_library::state::LibraryState;
    use agent_library::template::command::TemplateCommand;
    use agent_secret_manager::service::Service;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
        Router,
    };
    use serde_json::Value;
    use std::sync::Arc;
    use tower::Service as _;

    async fn create_public_offer_request(offer_id: &str, template_id: &str) -> Request<Body> {
        Request::builder()
            .method(http::Method::POST)
            .uri(format!("{API_VERSION}/create-public-offer"))
            .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "offerId": offer_id,
                    "templateId": template_id,
                }))
                .unwrap(),
            ))
            .unwrap()
    }

    async fn get_all_public_offers_request() -> Request<Body> {
        Request::builder()
            .method(http::Method::GET)
            .uri(format!("{API_VERSION}/get-all-public-offers"))
            .body(Body::empty())
            .unwrap()
    }

    async fn setup_app() -> (Router, Arc<LibraryState>) {
        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&issuance_state).await.unwrap();

        let library_state = setup_library_state(&issuance_state).await;
        create_test_template_with_auth(&library_state, true).await;

        (router((issuance_state, library_state.clone())), library_state)
    }

    /// Updates the schema of an existing template for public-offer schema validation tests.
    /// Uses `UpdateSchema` rather than `CreateNewTemplate` to preserve the template's Published
    /// status (required for credential creation) while setting the schema under test.
    async fn update_template_schema(library_state: &Arc<LibraryState>, template_id: &str, schema: Option<Value>) {
        let Some(schema) = schema else { return };
        let command = TemplateCommand::UpdateSchema {
            template_id: template_id.to_string(),
            schema,
        };

        command_handler(template_id, &library_state.command.template, command)
            .await
            .unwrap();
    }

    fn const_only_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "credentialSubject": {
                    "type": "object",
                    "properties": {
                        "name": { "const": "Alice" }
                    }
                }
            }
        })
    }

    fn non_const_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "credentialSubject": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                }
            }
        })
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_get_all_public_offers_returns_empty_list() {
        let (mut app, _library_state) = setup_app().await;

        let response = app.call(get_all_public_offers_request().await).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert!(list.is_empty());
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_create_public_offer_succeeds() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(const_only_schema())).await;

        // A credential (and its associated offer) must exist before creating a public offer.
        credentials(&mut app).await;

        let response = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // Should appear as Active in the list.
        let response = app.call(get_all_public_offers_request().await).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["status"], "Active");
        assert_eq!(list[0]["id"], crate::tests::OFFER_ID);
        assert_eq!(list[0]["templateId"], TEMPLATE_ID);
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_create_public_offer_fails_when_underlying_offer_not_found() {
        let (mut app, _library_state) = setup_app().await;

        let response = app
            .call(create_public_offer_request("nonexistent-offer", TEMPLATE_ID).await)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_create_public_offer_fails_when_already_exists() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(const_only_schema())).await;

        credentials(&mut app).await;

        let first = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);

        // Second create on the same offer ID must fail with 409 Conflict.
        let second = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::CONFLICT);
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_take_public_offer_offline() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(const_only_schema())).await;

        credentials(&mut app).await;
        let _ = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/take-public-offer-offline"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "offerId": crate::tests::OFFER_ID })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app.call(get_all_public_offers_request().await).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(list[0]["status"], "Inactive");
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_take_public_offer_online() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(const_only_schema())).await;

        credentials(&mut app).await;
        let _ = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();

        // Take offline first.
        app.call(
            Request::builder()
                .method(http::Method::POST)
                .uri(format!("{API_VERSION}/take-public-offer-offline"))
                .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "offerId": crate::tests::OFFER_ID })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

        // Then bring back online.
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/take-public-offer-online"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "offerId": crate::tests::OFFER_ID })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app.call(get_all_public_offers_request().await).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(list[0]["status"], "Active");
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_delete_public_offer_removes_from_list() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(const_only_schema())).await;

        credentials(&mut app).await;
        let _ = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/delete-public-offer"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "offerId": crate::tests::OFFER_ID })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app.call(get_all_public_offers_request().await).await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let list: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert!(list.is_empty(), "deleted offer must not appear in list");
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_take_public_offer_offline_returns_not_found_for_unknown_offer() {
        let (mut app, _library_state) = setup_app().await;

        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/take-public-offer-offline"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "offerId": "nonexistent" })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_create_public_offer_fails_when_template_not_found() {
        let (mut app, _library_state) = setup_app().await;

        credentials(&mut app).await;

        let response = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, "missing-template").await)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_create_public_offer_fails_when_template_schema_has_non_const_leaf() {
        let (mut app, library_state) = setup_app().await;

        update_template_schema(&library_state, TEMPLATE_ID, Some(non_const_schema())).await;
        credentials(&mut app).await;

        let response = app
            .call(create_public_offer_request(crate::tests::OFFER_ID, TEMPLATE_ID).await)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
