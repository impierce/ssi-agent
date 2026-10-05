pub mod openapi;
pub mod queries;
use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::{caller, command_handler};
use crate::API_VERSION;
use agent_library::catalog::{
    aggregate::{CatalogDisplay, CatalogVisibility},
    command::CatalogCommand,
    error::CatalogError,
    views::CatalogView,
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};

use agent_library::queries::get_catalog;
use agent_library::state::LibraryState;
use http::StatusCode;
use http_api_problem::ApiError;
use hyper::header;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
use std::sync::Arc;

/// Data transfer object for Catalogs.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Catalog)]
pub struct CatalogDto {
    #[serde(rename = "id")]
    pub catalog_id: String,
    pub display: CatalogDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogVisibility,
    pub modified_at: DateTime<Utc>,
}

impl From<CatalogView> for CatalogDto {
    fn from(v: CatalogView) -> Self {
        Self {
            catalog_id: v.catalog_id,
            display: v.display,
            template_ids: v.template_ids,
            visibility: v.visibility,
            modified_at: v.modified_at,
        }
    }
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CreateCatalogRequest {
    pub display: CatalogDisplay,
    pub visibility: CatalogVisibility,
}

/// Create a new catalog
///
/// Creates a new catalog with the provided display information.
#[utoipa::path(
    post,
    path = "/create-new-catalog",
    operation_id = "create_new_catalog",
    tags = ["Library", "Catalog"],
    request_body(
        content = CreateCatalogRequest,
        ),
    responses(
        (status = 201, description = "Catalog created successfully", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body, or an empty catalog name"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn create_catalog(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(CreateCatalogRequest { display, visibility }): Json<CreateCatalogRequest>,
) -> Result<Response, ApiError> {
    let catalog_id = uuid::Uuid::new_v4().to_string();

    if display.name.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST));
    }

    let command = CatalogCommand::CreateCatalog {
        catalog_id: catalog_id.clone(),
        display,
        visibility,
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the created catalog
    get_catalog(&state, Caller::Internal, &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, format!("{API_VERSION}/catalog/{catalog_id}"))],
                Json(CatalogDto::from(catalog_view)),
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct AddTemplatesRequest {
    pub catalog_id: String,
    pub template_ids: Vec<String>,
}

/// Add one or more templates to a catalog
///
/// Adds one or more templates to a catalog by their IDs.
#[utoipa::path(
    post,
    path = "/add-templates-to-catalog",
    operation_id = "add_templates_to_catalog",
    tags = ["Library", "Catalog"],
    request_body(
        content = AddTemplatesRequest,
        ),
    responses(
        (status = 200, description = "Catalog updated successfully", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn add_templates_to_catalog(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(AddTemplatesRequest {
        catalog_id,
        template_ids,
    }): Json<AddTemplatesRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::AddTemplateIds {
        catalog_id: catalog_id.clone(),
        template_ids,
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the updated catalog
    get_catalog(&state, Caller::Internal, &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct RemoveTemplatesRequest {
    pub catalog_id: String,
    pub template_ids: Vec<String>,
}

/// Remove one or more templates from a catalog
///
/// Removes one or more templates from a catalog by their ID.
#[utoipa::path(
    post,
    path = "/remove-templates-from-catalog",
    operation_id = "remove-templates-from-catalog",
    tags = ["Library", "Catalog"],
    request_body(
        content = RemoveTemplatesRequest,
        ),
    responses(
        (status = 200, description = "Template(s) removed successfully", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn remove_templates_from_catalog(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(RemoveTemplatesRequest {
        catalog_id,
        template_ids,
    }): Json<RemoveTemplatesRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::RemoveTemplateIds {
        catalog_id: catalog_id.clone(),
        template_ids,
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the updated catalog
    get_catalog(&state, Caller::Internal, &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct ChangeCatalogAppearanceRequest {
    pub catalog_id: String,
    pub display: CatalogDisplay,
}

/// Changes a catalog's display information
///
/// Changes a catalog's display information such as name, description, and icon.
#[utoipa::path(
    post,
    path = "/change-catalog-appearance",
    operation_id = "change_catalog_appearance",
    tags = ["Library", "Catalog"],
    request_body(
        content = ChangeCatalogAppearanceRequest,
        ),
    responses(
        (status = 200, description = "Catalog appearance updated successfully", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn change_catalog_appearance(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(ChangeCatalogAppearanceRequest { catalog_id, display }): Json<ChangeCatalogAppearanceRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::ChangeCatalogAppearance {
        catalog_id: catalog_id.clone(),
        display: display.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the updated catalog
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct MakeCatalogPublicRequest {
    pub catalog_id: String,
}

/// Make catalog public
///
/// Updates a catalog's visibility to public.
#[utoipa::path(
    post,
    path = "/make-catalog-public",
    operation_id = "make_catalog_public",
    tags = ["Library", "Catalog"],
    request_body(
        content = MakeCatalogPublicRequest,
        ),
    responses(
        (status = 200, description = "Catalog made public successfully", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn make_catalog_public(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(MakeCatalogPublicRequest { catalog_id }): Json<MakeCatalogPublicRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::MakeCatalogPublic {
        catalog_id: catalog_id.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the updated catalog
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct MakeCatalogPrivateRequest {
    pub catalog_id: String,
}

/// Make catalog private
///
/// Updates a catalog's visibility to private. If not otherwise specified, the default visibility is private.
#[utoipa::path(
    post,
    path = "/make-catalog-private",
    operation_id = "make_catalog_private",
    tags = ["Library", "Catalog"],
    request_body(
        content = MakeCatalogPrivateRequest,
        ),
    responses(
        (status = 200, description = "Catalog made private successfully.", body = CatalogDto),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn make_catalog_private(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(MakeCatalogPrivateRequest { catalog_id }): Json<MakeCatalogPrivateRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::MakeCatalogPrivate {
        catalog_id: catalog_id.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;

    // Return the updated catalog
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .map(|catalog_view| (StatusCode::OK, Json(CatalogDto::from(catalog_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct DeleteCatalogRequest {
    pub catalog_id: String,
}

/// Delete a catalog
///
/// Deletes a catalog.
#[utoipa::path(
    post,
    path = "/delete-catalog",
    operation_id = "delete_catalog",
    tags = ["Library", "Catalog"],
    request_body(
        content = DeleteCatalogRequest,
        ),
    responses(
        (status = 204, description = "Catalog deleted"),
        (status = 400, description = "Malformed JSON request body"),
        (status = 404, description = "Catalog not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn delete_catalog(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(DeleteCatalogRequest { catalog_id }): Json<DeleteCatalogRequest>,
) -> Result<Response, ApiError> {
    get_catalog(&state, caller(actor.clone()), &catalog_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| CatalogError::CatalogNotFound(catalog_id.clone()).into_api_error())?;

    let command = CatalogCommand::DeleteCatalog {
        catalog_id: catalog_id.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.command.catalog,
        command,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::public_command_handler;
    use agent_store::{in_memory::InMemory, library_state};
    use shared_kernel::{
        async_trait,
        authorization::{
            Actor, AuthorizationChecker, AuthorizationError, AuthorizationOperation, AuthorizationRequest, Caller,
        },
    };
    use std::sync::Mutex;

    struct CapturingAuthorizationChecker {
        requests: Arc<Mutex<Vec<AuthorizationRequest>>>,
    }

    #[async_trait]
    impl AuthorizationChecker for CapturingAuthorizationChecker {
        async fn is_authorized(&self, request: &AuthorizationRequest) -> Result<(), AuthorizationError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(())
        }
    }

    async fn catalog_state(requests: Arc<Mutex<Vec<AuthorizationRequest>>>, catalog_id: &str) -> Arc<LibraryState> {
        let mut state = library_state(&InMemory, &Default::default(), Default::default(), Default::default()).await;

        public_command_handler(
            catalog_id,
            &state.command.catalog,
            CatalogCommand::CreateCatalog {
                catalog_id: catalog_id.to_string(),
                display: CatalogDisplay {
                    name: "Catalog".to_string(),
                    description: String::new(),
                    icon: None,
                },
                visibility: CatalogVisibility::Private,
            },
        )
        .await
        .unwrap();

        state.authorization_checker = Arc::new(CapturingAuthorizationChecker { requests });
        Arc::new(state)
    }

    fn assert_catalog_mutation_sequence(
        requests: &[AuthorizationRequest],
        actor: &Actor,
        catalog_id: &str,
        command_operation_name: &'static str,
    ) {
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].caller, Caller::Actor(actor.clone()));
        assert_eq!(requests[1].caller, Caller::Actor(actor.clone()));
        assert_eq!(requests[2].caller, Caller::Internal);

        assert_eq!(
            requests[0].operation,
            AuthorizationOperation::Query {
                resource_id: Some(catalog_id.to_string()),
                operation_name: "library.catalogs.get",
            }
        );
        assert_eq!(
            requests[1].operation,
            AuthorizationOperation::Command {
                aggregate_id: catalog_id.to_string(),
                resource_id: None,
                operation_name: command_operation_name,
            }
        );
        assert_eq!(
            requests[2].operation,
            AuthorizationOperation::Query {
                resource_id: Some(catalog_id.to_string()),
                operation_name: "library.catalogs.get",
            }
        );
    }

    #[tokio::test]
    async fn catalog_template_mutations_preserve_the_authorization_boundary() {
        let catalog_id = "catalog-1";
        let actor = Actor {
            subject: "user@example.test".to_string(),
        };

        for (operation_name, add) in [
            ("library.catalogs.templates.add", true),
            ("library.catalogs.templates.remove", false),
        ] {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let state = catalog_state(Arc::clone(&requests), catalog_id).await;

            if add {
                add_templates_to_catalog(
                    State(state),
                    RequestActor(Some(actor.clone())),
                    Json(AddTemplatesRequest {
                        catalog_id: catalog_id.to_string(),
                        template_ids: Vec::new(),
                    }),
                )
                .await
                .unwrap();
            } else {
                remove_templates_from_catalog(
                    State(state),
                    RequestActor(Some(actor.clone())),
                    Json(RemoveTemplatesRequest {
                        catalog_id: catalog_id.to_string(),
                        template_ids: Vec::new(),
                    }),
                )
                .await
                .unwrap();
            }

            assert_catalog_mutation_sequence(&requests.lock().unwrap(), &actor, catalog_id, operation_name);
        }
    }

    mod endpoints {
        use super::*;
        use crate::{tests::TEMPLATE_ID, v0::issuance::credentials::tests::create_test_template};
        use axum::{
            body::{to_bytes, Body},
            extract::Request,
            Router,
        };
        use serde_json::{json, Value};
        use tower::ServiceExt;

        async fn setup() -> Router {
            let state = Arc::new(library_state(&InMemory, &Default::default(), Default::default(), vec![]).await);
            create_test_template(&state).await;

            crate::v0::library::router(state)
        }

        async fn send(app: &Router, request: Request<Body>) -> (StatusCode, http::HeaderMap, Value) {
            let response = app.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

            (status, headers, serde_json::from_slice(&body).unwrap_or(Value::Null))
        }

        async fn post(app: &Router, path: &str, body: Value) -> (StatusCode, Value) {
            let (status, _, body) = send(
                app,
                Request::post(format!("{API_VERSION}{path}"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await;

            (status, body)
        }

        async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
            let (status, _, body) = send(
                app,
                Request::get(format!("{API_VERSION}{path}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;

            (status, body)
        }

        async fn create(app: &Router, name: &str) -> String {
            let (status, headers, catalog) = send(
                app,
                Request::post(format!("{API_VERSION}/create-new-catalog"))
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "display": { "name": name, "description": "Description" } }).to_string(),
                    ))
                    .unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);

            let catalog_id = catalog["id"].as_str().unwrap().to_string();
            assert_eq!(
                headers[header::LOCATION],
                format!("{API_VERSION}/catalog/{catalog_id}").as_str()
            );
            assert_eq!(catalog["display"]["name"], name);
            assert_eq!(catalog["visibility"], "Private");
            assert_eq!(catalog["template_ids"], json!([]));

            catalog_id
        }

        #[tokio::test]
        async fn catalog_lifecycle() {
            let app = setup().await;
            let catalog_id = create(&app, "Catalog").await;

            let (status, catalog) = get(&app, &format!("/get-catalog-by-id/{catalog_id}")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["id"], catalog_id.as_str());

            let (status, catalog) = post(
                &app,
                "/add-templates-to-catalog",
                json!({ "catalogId": catalog_id, "templateIds": [TEMPLATE_ID] }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["template_ids"], json!([TEMPLATE_ID]));

            let (status, catalog) = post(
                &app,
                "/remove-templates-from-catalog",
                json!({ "catalogId": catalog_id, "templateIds": [TEMPLATE_ID] }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["template_ids"], json!([]));

            let (status, catalog) = post(
                &app,
                "/change-catalog-appearance",
                json!({ "catalogId": catalog_id, "display": { "name": "Renamed", "description": "Other" } }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["display"]["name"], "Renamed");
            assert_eq!(catalog["display"]["description"], "Other");

            let (status, catalog) = post(&app, "/make-catalog-public", json!({ "catalogId": catalog_id })).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["visibility"], "Public");

            let (status, catalog) = post(&app, "/make-catalog-private", json!({ "catalogId": catalog_id })).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalog["visibility"], "Private");

            let (status, catalogs) = get(&app, "/get-all-catalogs").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalogs.as_array().unwrap().len(), 1);
            assert_eq!(catalogs[0]["id"], catalog_id.as_str());

            let (status, _) = post(&app, "/delete-catalog", json!({ "catalogId": catalog_id })).await;
            assert_eq!(status, StatusCode::NO_CONTENT);

            let (status, _) = get(&app, &format!("/get-catalog-by-id/{catalog_id}")).await;
            assert_eq!(status, StatusCode::NOT_FOUND);

            let (status, catalogs) = get(&app, "/get-all-catalogs").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalogs, json!([]));
        }

        #[tokio::test]
        async fn get_all_catalogs_is_empty_without_catalogs() {
            let app = setup().await;

            let (status, catalogs) = get(&app, "/get-all-catalogs").await;

            assert_eq!(status, StatusCode::OK);
            assert_eq!(catalogs, json!([]));
        }

        #[tokio::test]
        async fn empty_catalog_names_are_rejected() {
            let app = setup().await;

            let (status, _) = post(
                &app,
                "/create-new-catalog",
                json!({ "display": { "name": "  ", "description": "" } }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);

            let catalog_id = create(&app, "Catalog").await;
            let (status, _) = post(
                &app,
                "/change-catalog-appearance",
                json!({ "catalogId": catalog_id, "display": { "name": "", "description": "" } }),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn adding_an_unknown_template_is_rejected() {
            let app = setup().await;
            let catalog_id = create(&app, "Catalog").await;

            let (status, _) = post(
                &app,
                "/add-templates-to-catalog",
                json!({ "catalogId": catalog_id, "templateIds": ["unknown-template"] }),
            )
            .await;

            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn adding_a_deleted_template_is_rejected() {
            let app = setup().await;
            let catalog_id = create(&app, "Catalog").await;
            let (status, _) = post(
                &app,
                "/update-template",
                json!({ "id": TEMPLATE_ID, "status": "archived" }),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            let (status, _) = post(&app, "/delete-template", json!({ "id": TEMPLATE_ID })).await;
            assert_eq!(status, StatusCode::NO_CONTENT);

            let (status, _) = post(
                &app,
                "/add-templates-to-catalog",
                json!({ "catalogId": catalog_id, "templateIds": [TEMPLATE_ID] }),
            )
            .await;

            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn mutations_on_unknown_or_deleted_catalogs_return_not_found() {
            let app = setup().await;
            let deleted_catalog_id = create(&app, "Catalog").await;
            let (status, _) = post(&app, "/delete-catalog", json!({ "catalogId": deleted_catalog_id })).await;
            assert_eq!(status, StatusCode::NO_CONTENT);

            for catalog_id in ["unknown-catalog", deleted_catalog_id.as_str()] {
                for (path, body) in [
                    (
                        "/add-templates-to-catalog",
                        json!({ "catalogId": catalog_id, "templateIds": [TEMPLATE_ID] }),
                    ),
                    (
                        "/remove-templates-from-catalog",
                        json!({ "catalogId": catalog_id, "templateIds": [TEMPLATE_ID] }),
                    ),
                    (
                        "/change-catalog-appearance",
                        json!({ "catalogId": catalog_id, "display": { "name": "Renamed", "description": "" } }),
                    ),
                    ("/make-catalog-public", json!({ "catalogId": catalog_id })),
                    ("/make-catalog-private", json!({ "catalogId": catalog_id })),
                    ("/delete-catalog", json!({ "catalogId": catalog_id })),
                ] {
                    let (status, _) = post(&app, path, body).await;
                    assert_eq!(status, StatusCode::NOT_FOUND, "{path} for catalog '{catalog_id}'");
                }
            }
        }
    }
}
