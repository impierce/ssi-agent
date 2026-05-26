pub mod openapi;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::catalogue::{
    aggregate::{CatalogueDisplay, CatalogueVisibility},
    command::CatalogueCommand,
};

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};

use http::StatusCode;
use http_api_problem::ApiError;
use hyper::header;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use agent_library::state::LibraryState;

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CreateCatalogueRequest {
    pub display: CatalogueDisplay,
    pub visibility: CatalogueVisibility,
}

/// Create a new catalogue
///
/// Creates a new catalogue with the provided display information.
#[utoipa::path(
    post,
    path = "/catalogue/create-catalogue",
    tags = ["Library", "Catalogue"],
    request_body(
        content = CreateCatalogueRequest,
        )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn create_catalogue(
    State(state): State<Arc<LibraryState>>,
    Json(CreateCatalogueRequest { display, visibility }): Json<CreateCatalogueRequest>,
) -> Result<Response, ApiError> {
    let catalogue_id = uuid::Uuid::new_v4().to_string();

    let command = CatalogueCommand::CreateCatalogue {
        catalogue_id: catalogue_id.clone(),
        display,
        visibility,
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;

    // Return the created catalogue
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .map(|catalogue_view| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, format!("{API_VERSION}/catalogue/{catalogue_id}"))],
                Json(catalogue_view),
            )
                .into_response()
        })
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct AddTemplateRequest {
    pub catalogue_id: String,
    pub template_id: String,
}

/// Add a template to a catalogue
///
/// Adds a template to a catalogue by its ID.
#[utoipa::path(
    post,
    path = "/catalogue/add-template",
    tags = ["Library", "Catalogue"],
    request_body(
        content = AddTemplateRequest,
        )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn add_template(
    State(state): State<Arc<LibraryState>>,
    Json(AddTemplateRequest {
        catalogue_id,
        template_id,
    }): Json<AddTemplateRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogueCommand::AddTemplateId {
        catalogue_id: catalogue_id.clone(),
        template_id,
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;

    // Return the updated catalogue
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .map(|catalogue_view| (StatusCode::OK, Json(catalogue_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct RemoveTemplateRequest {
    pub catalogue_id: String,
    pub template_id: String,
}

/// Remove a template from a catalogue
///
/// Removes a template from a catalogue by its ID.
#[utoipa::path(
    post,
    path = "/catalogue/remove-template",
    tags = ["Library", "Catalogue"],
    request_body(
        content = RemoveTemplateRequest,
        )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn remove_template(
    State(state): State<Arc<LibraryState>>,
    Json(RemoveTemplateRequest {
        catalogue_id,
        template_id,
    }): Json<RemoveTemplateRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogueCommand::RemoveTemplateId {
        catalogue_id: catalogue_id.clone(),
        template_id,
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;

    // Return the updated catalogue
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .map(|catalogue_view| (StatusCode::OK, Json(catalogue_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCatalogueDisplayRequest {
    pub catalogue_id: String,
    pub display: CatalogueDisplay,
}

/// Update a catalogue's display information
///
/// Updates a catalogue's display information such as name, description, and icon.
#[utoipa::path(
    post,
    path = "/catalogue/update-display",
    tags = ["Library", "Catalogue"],
    request_body(
        content = UpdateCatalogueDisplayRequest,
        )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn update_display(
    State(state): State<Arc<LibraryState>>,
    Json(UpdateCatalogueDisplayRequest { catalogue_id, display }): Json<UpdateCatalogueDisplayRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogueCommand::UpdateDisplay {
        catalogue_id: catalogue_id.clone(),
        display,
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;

    // Return the updated catalogue
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .map(|catalogue_view| (StatusCode::OK, Json(catalogue_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCatalogueVisibilityRequest {
    pub catalogue_id: String,
    pub visibility: CatalogueVisibility,
}

/// Update a catalogue's visibility
///
/// Updates a catalogue's visibility (e.g., public, private, draft).
#[utoipa::path(
    post,
    path = "/catalogue/update-visibility",
    tags = ["Library", "Catalogue"],
    request_body(
        content = UpdateCatalogueVisibilityRequest,
        )
    )]
#[axum_macros::debug_handler]
pub(crate) async fn update_visibility(
    State(state): State<Arc<LibraryState>>,
    Json(UpdateCatalogueVisibilityRequest {
        catalogue_id,
        visibility,
    }): Json<UpdateCatalogueVisibilityRequest>,
) -> Result<Response, ApiError> {
    let command = CatalogueCommand::UpdateVisibility {
        catalogue_id: catalogue_id.clone(),
        visibility,
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;

    // Return the updated catalogue
    query_handler(&catalogue_id, &state.query.catalogue)
        .await?
        .map(|catalogue_view| (StatusCode::OK, Json(catalogue_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

/// Delete a catalogue
///
/// Deletes a catalogue.
#[utoipa::path(
    post,
    path = "/catalogue/delete-catalogue/{catalogue_id}",
    tags = ["Library", "Catalogue"],
    )]
#[axum_macros::debug_handler]
pub(crate) async fn delete_catalogue(
    State(state): State<Arc<LibraryState>>,
    Path(catalogue_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = CatalogueCommand::DeleteCatalogue {
        catalogue_id: catalogue_id.clone(),
    };

    command_handler(&catalogue_id, &state.command.catalogue, command).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
