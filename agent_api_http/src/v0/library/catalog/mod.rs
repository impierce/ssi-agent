pub mod openapi;
pub mod queries;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::catalog::{
    aggregate::{CatalogDisplay, CatalogVisibility},
    command::CatalogCommand,
};
use axum::{
    extract::{Path, State},
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
#[serde(rename_all = "camelCase")]
#[schema(as = Catalog)]
pub struct Catalog {
    #[serde(rename = "id")]
    pub catalog_id: String,
    pub display: CatalogDisplay,
    pub template_ids: Vec<String>,
    pub visibility: CatalogVisibility,
    pub modified_at: DateTime<Utc>,
    pub is_deleted: bool,
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
    path = "/catalog/create-catalog",
    tags = ["Library", "Catalog"],
    request_body(
        content = CreateCatalogRequest,
        ),
    responses(
        (status = 201, description = "Catalog created successfully", body = Catalog)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn create_catalog(
    State(state): State<Arc<LibraryState>>,
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

    command_handler(&catalog_id, &state.command.catalog, command).await?;

    // Return the created catalog
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .map(|catalog_view| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, format!("{API_VERSION}/catalog/{catalog_id}"))],
                Json(catalog_view),
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
    path = "/catalog/add-templates",
    tags = ["Library", "Catalog"],
    request_body(
        content = AddTemplatesRequest,
        ),
    responses(
        (status = 200, description = "Catalog updated successfully", body = Catalog)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn add_templates(
    State(state): State<Arc<LibraryState>>,
    Json(AddTemplatesRequest {
        catalog_id,
        template_ids,
    }): Json<AddTemplatesRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogCommand::AddTemplateIds {
        catalog_id: catalog_id.clone(),
        template_ids,
    };

    command_handler(&catalog_id, &state.command.catalog, command).await?;

    // Return the updated catalog
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .map(|catalog_view| (StatusCode::OK, Json(catalog_view)).into_response())
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
    path = "/catalog/remove-templates",
    tags = ["Library", "Catalog"],
    request_body(
        content = RemoveTemplatesRequest,
        ),
    responses(
        (status = 200, description = "Template(s) removed successfully", body = Catalog)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn remove_templates(
    State(state): State<Arc<LibraryState>>,
    Json(RemoveTemplatesRequest {
        catalog_id,
        template_ids,
    }): Json<RemoveTemplatesRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogCommand::RemoveTemplateIds {
        catalog_id: catalog_id.clone(),
        template_ids,
    };

    command_handler(&catalog_id, &state.command.catalog, command).await?;

    // Return the updated catalog
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .map(|catalog_view| (StatusCode::OK, Json(catalog_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCatalogDisplayRequest {
    pub catalog_id: String,
    pub display: CatalogDisplay,
}

/// Update a catalog's display information
///
/// Updates a catalog's display information such as name, description, and icon.
#[utoipa::path(
    post,
    path = "/catalog/update-display",
    tags = ["Library", "Catalog"],
    request_body(
        content = UpdateCatalogDisplayRequest,
        ),
    responses(
        (status = 200, description = "Catalog display updated successfully", body = Catalog)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn update_display(
    State(state): State<Arc<LibraryState>>,
    Json(UpdateCatalogDisplayRequest { catalog_id, display }): Json<UpdateCatalogDisplayRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogCommand::UpdateDisplay {
        catalog_id: catalog_id.clone(),
        display: display.clone(),
    };

    if display.name.trim().is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST));
    }

    command_handler(&catalog_id, &state.command.catalog, command).await?;

    // Return the updated catalog
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .map(|catalog_view| (StatusCode::OK, Json(catalog_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCatalogVisibilityRequest {
    pub catalog_id: String,
    pub visibility: CatalogVisibility,
}

/// Update a catalog's visibility
///
/// Updates a catalog's visibility (e.g., public, private, draft).
#[utoipa::path(
    post,
    path = "/catalog/update-visibility",
    tags = ["Library", "Catalog"],
    request_body(
        content = UpdateCatalogVisibilityRequest,
        ),
    responses(
        (status = 200, description = "Catalog visibility updated successfully", body = Catalog)
    )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn update_visibility(
    State(state): State<Arc<LibraryState>>,
    Json(UpdateCatalogVisibilityRequest { catalog_id, visibility }): Json<UpdateCatalogVisibilityRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogCommand::UpdateVisibility {
        catalog_id: catalog_id.clone(),
        visibility,
    };

    command_handler(&catalog_id, &state.command.catalog, command).await?;

    // Return the updated catalog
    query_handler(&catalog_id, &state.query.catalog)
        .await?
        .map(|catalog_view| (StatusCode::OK, Json(catalog_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Delete a catalog
///
/// Deletes a catalog.
#[utoipa::path(
    post,
    path = "/catalog/delete-catalog/{catalog_id}",
    tags = ["Library", "Catalog"],
    )]
#[axum_macros::debug_handler]
pub(crate) async fn delete_catalog(
    State(state): State<Arc<LibraryState>>,
    Path(catalog_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = CatalogCommand::DeleteCatalog {
        catalog_id: catalog_id.clone(),
    };

    command_handler(&catalog_id, &state.command.catalog, command).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
