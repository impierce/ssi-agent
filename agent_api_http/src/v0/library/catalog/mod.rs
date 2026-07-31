pub mod openapi;
pub mod queries;
use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::{command_handler, internal_query_handler, query_handler};
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

use agent_library::state::LibraryState;
use http::StatusCode;
use http_api_problem::ApiError;
use hyper::header;
use serde::{Deserialize, Serialize};
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
        (status = 201, description = "Catalog created successfully", body = CatalogDto)
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
    internal_query_handler(state.authorization_checker.clone(), &catalog_id, &state.query.catalog)
        .await?
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
        (status = 200, description = "Catalog updated successfully", body = CatalogDto)
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
    internal_query_handler(state.authorization_checker.clone(), &catalog_id, &state.query.catalog)
        .await?
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
    internal_query_handler(state.authorization_checker.clone(), &catalog_id, &state.query.catalog)
        .await?
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
        (status = 200, description = "Template(s) removed successfully", body = CatalogDto)
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
    internal_query_handler(state.authorization_checker.clone(), &catalog_id, &state.query.catalog)
        .await?
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
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
        (status = 200, description = "Catalog appearance updated successfully", body = CatalogDto)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn change_catalog_appearance(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(ChangeCatalogAppearanceRequest { catalog_id, display }): Json<ChangeCatalogAppearanceRequest>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
        (status = 200, description = "Catalog make public successfully", body = CatalogDto)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn make_catalog_public(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(MakeCatalogPublicRequest { catalog_id }): Json<MakeCatalogPublicRequest>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
    Ok(StatusCode::OK.into_response())
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
        (status = 200, description = "Catalog made private successfully.", body = CatalogDto)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn make_catalog_private(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(MakeCatalogPrivateRequest { catalog_id }): Json<MakeCatalogPrivateRequest>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
    Ok(StatusCode::OK.into_response())
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
        (status = 204, description = "Catalog deleted"))
    )]
#[axum_macros::debug_handler]
pub(crate) async fn delete_catalog(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(DeleteCatalogRequest { catalog_id }): Json<DeleteCatalogRequest>,
) -> Result<Response, ApiError> {
    query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &catalog_id,
        &state.query.catalog,
    )
    .await?
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
