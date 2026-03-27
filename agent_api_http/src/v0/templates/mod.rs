use crate::error::type_url;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{DataModel, Display, HolderType, Status, Template, Visibility};
use agent_library::template::command::TemplateCommand;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub mod openapi;

/// Data transfer object for Templates.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Template)]
pub struct TemplateDto {
    #[serde(rename = "id")]
    pub template_id: String,
    pub source_template_id: Option<String>,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub data_model: Option<DataModel>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub modified_at: Option<String>,
    pub tags: Vec<String>,
    pub status: Status,
    pub visibility: Visibility,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

impl From<Template> for TemplateDto {
    fn from(value: Template) -> Self {
        Self {
            template_id: value.template_id,
            source_template_id: value.source_template_id,
            title: value.title,
            display: value.display,
            data_model: value.data_model,
            creator: value.creator,
            holder_type: value.holder_type,
            modified_at: value.modified_at,
            tags: value.tags,
            status: value.status,
            visibility: value.visibility,
            description: value.description,
            r#type: value.r#type,
            schema: *value.schema,
        }
    }
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct CreateTemplateEndpointRequest {
    pub title: Option<String>,
    pub display: Option<Display>,
    pub data_model: Option<DataModel>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub tags: Vec<String>,
    pub status: Status,
    pub visibility: Visibility,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

/// Create a new template
///
/// Creates a new template which can be used to issue credentials.
#[utoipa::path(
    post,
    path = "/templates/create-template",
    tags = ["Library", "Templates"],
    request_body(
        content = CreateTemplateEndpointRequest,
        examples(
            ("Standard template" = (
                description = "A simple example that will issue credentials in the W3C Verifiable Credentials Data Model v1.1 format.",
                value = json!({ "title": "Standard template", "dataModel": "w3c_vc_data_model_v1-1", "holderType": "individual" })
            ))
        )
    ),
    responses(
        (status = 201, description = "New template created successfully", headers(("Location", description = "The path of the newly created template")), body = TemplateDto)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_template(
    State(state): State<Arc<LibraryState>>,
    Json(CreateTemplateEndpointRequest {
        title,
        display,
        data_model,
        creator,
        holder_type,
        tags,
        status,
        visibility,
        description,
        r#type,
        schema,
    }): Json<CreateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    let template_id = Uuid::new_v4().to_string();

    let command = TemplateCommand::CreateTemplate {
        template_id: template_id.clone(),
        source_template_id: None,
        title,
        display,
        data_model,
        creator,
        holder_type,
        tags,
        status,
        visibility,
        description,
        r#type,
        schema: Box::new(schema),
    };

    command_handler(&template_id, &state.command.template, command).await?;

    // Return the template.
    query_handler(&template_id, &state.query.template)
        .await?
        .map(|template_view| {
            (
                StatusCode::CREATED,
                [(header::LOCATION, &format!("{API_VERSION}/templates/{template_id}"))],
                Json(TemplateDto::from(template_view)),
            )
                .into_response()
        })
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateTemplateEndpointRequest {
    pub source_template_id: String,
}

/// Duplicate existing template
///
/// Creates a duplicate of an existing template.
#[utoipa::path(
    post,
    path = "/templates/duplicate-template",
    tags = ["Library", "Templates"],
    request_body(
        content = DuplicateTemplateEndpointRequest,
        example = json!({ "sourceTemplateId": "91fc790f-d876-4827-9a9d-0fb0f6766dca" })
    ),
    responses(
        (status = 201, description = "Duplicate created successfully", headers(("Location", description = "The path of the newly created template")), body = TemplateDto),
        (status = 422, description = "Source Template Not Found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn duplicate_template(
    State(state): State<Arc<LibraryState>>,
    Json(DuplicateTemplateEndpointRequest { source_template_id }): Json<DuplicateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    let new_template_id = Uuid::new_v4().to_string();

    let original_template = query_handler(&source_template_id, &state.query.template)
        .await?
        .ok_or_else(|| {
            ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                .title("Source Template Not Found")
                .type_url(type_url("library#source-template-not-found"))
                .message(format!("No Source Template found with id: `{source_template_id}`"))
                .finish()
        })?;

    let command = TemplateCommand::CreateTemplate {
        template_id: new_template_id.clone(),
        source_template_id: Some(source_template_id),
        title: original_template.title.map(|t| format!("{} Copy", t)),
        display: original_template.display,
        data_model: original_template.data_model,
        creator: original_template.creator,
        holder_type: original_template.holder_type,
        tags: original_template.tags,
        status: Status::Draft,
        visibility: original_template.visibility,
        description: original_template.description,
        r#type: original_template.r#type,
        schema: original_template.schema,
    };

    command_handler(&new_template_id, &state.command.template, command).await?;

    // Return the duplicated template.
    let new_template = query_handler(&new_template_id, &state.query.template)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, &format!("{API_VERSION}/templates/{new_template_id}"))],
        Json(new_template),
    )
        .into_response())
}

#[derive(Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateTemplateEndpointRequest {
    #[serde(rename = "id")]
    pub template_id: String,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub data_model: Option<DataModel>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub tags: Vec<String>,
    pub status: Option<Status>,
    pub visibility: Option<Visibility>,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

/// Update a template
///
/// Updates an existing template with the provided content.
#[utoipa::path(
    post,
    path = "/templates/update-template",
    operation_id = "update_template",
    tags = ["Library", "Templates"],
    responses(
        (status = 204, description = "Template updated successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn update_template(
    State(state): State<Arc<LibraryState>>,
    Json(UpdateTemplateEndpointRequest {
        template_id,
        title,
        display,
        data_model,
        creator,
        holder_type,
        tags,
        status,
        visibility,
        description,
        r#type,
        schema,
    }): Json<UpdateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_id.is_empty() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Template ID Missing")
            .type_url(type_url("library#template-id-missing"))
            .message("The `id` field is required to update a template.")
            .finish());
    }

    query_handler(&template_id, &state.query.template)
        .await?
        .ok_or_else(|| {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .type_url(type_url("library#template-not-found"))
                .message(format!("No Template found with id: `{template_id}`"))
                .finish()
        })?;

    if let Some(title) = title {
        let command = TemplateCommand::UpdateTitle {
            template_id: template_id.clone(),
            title,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(display) = display {
        let command = TemplateCommand::UpdateDisplay {
            template_id: template_id.clone(),
            display,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(data_model) = data_model {
        let command = TemplateCommand::UpdateDataModel {
            template_id: template_id.clone(),
            data_model,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(creator) = creator {
        let command = TemplateCommand::UpdateCreator {
            template_id: template_id.clone(),
            creator,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(holder_type) = holder_type {
        let command = TemplateCommand::UpdateHolderType {
            template_id: template_id.clone(),
            holder_type,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if !tags.is_empty() {
        let command = TemplateCommand::UpdateTags {
            template_id: template_id.clone(),
            tags,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(status) = status {
        let command = TemplateCommand::UpdateStatus {
            template_id: template_id.clone(),
            status,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(visibility) = visibility {
        let command = TemplateCommand::UpdateVisibility {
            template_id: template_id.clone(),
            visibility,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(description) = description {
        let command = TemplateCommand::UpdateDescription {
            template_id: template_id.clone(),
            description,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if !r#type.is_empty() {
        let command = TemplateCommand::UpdateType {
            template_id: template_id.clone(),
            r#type,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(schema) = schema {
        let command = TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema,
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// List all templates
///
/// List all available templates.
#[utoipa::path(
    get,
    path = "/templates/get-all-templates",
    operation_id = "get_all_templates",
    tags = ["Library", "Templates"],
    responses(
        (status = 200, description = "All templates retrieved successfully", body = [TemplateDto])
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_templates(State(state): State<Arc<LibraryState>>) -> Result<Response, ApiError> {
    let filtered_templates = query_handler("all_templates", &state.query.all_templates)
        .await?
        .map(|all_templates_view| {
            let filtered_templates: Vec<TemplateDto> = all_templates_view
                .templates
                .into_values()
                .filter(|template| {
                    template.status != Status::Deleted
                    // TODO: Apply filtering logic based on request parameters
                })
                .map(TemplateDto::from)
                .collect();

            filtered_templates
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_templates)).into_response())
}

/// Get template by ID
///
/// Retrieve a specific template by its ID.
#[utoipa::path(
    get,
    path = "/templates/{template_id}",
    operation_id = "get_template_by_id",
    tags = ["Library", "Templates"],
    responses(
        (status = 200, description = "Template retrieved successfully", body = TemplateDto)
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_template(
    State(state): State<Arc<LibraryState>>,
    Path(template_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&template_id, &state.query.template)
        .await?
        .and_then(|template_view| {
            if template_view.status == Status::Deleted {
                None
            } else {
                Some(template_view)
            }
        })
        .map(|template_view| (StatusCode::OK, Json(TemplateDto::from(template_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteTemplateEndpointRequest {
    #[serde(rename = "id")]
    pub template_id: String,
}

/// Delete a template
///
/// Deletes a template by marking its status as `Deleted`. Deleted templates will no longer appear in any views.
#[utoipa::path(
    post,
    path = "/templates/delete-template",
    operation_id = "delete_template_by_id",
    tags = ["Library", "Templates"],
    responses(
        (status = 204, description = "Template deleted successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_template(
    State(state): State<Arc<LibraryState>>,
    Json(DeleteTemplateEndpointRequest { template_id }): Json<DeleteTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_id.is_empty() {
        return Err(ApiError::builder(StatusCode::BAD_REQUEST)
            .title("Template ID Missing")
            .type_url(type_url("library#template-id-missing"))
            .message("The `id` field is required to delete a template.")
            .finish());
    }

    query_handler(&template_id, &state.query.template)
        .await?
        .ok_or_else(|| {
            ApiError::builder(StatusCode::NOT_FOUND)
                .title("Template Not Found")
                .type_url(type_url("library#template-not-found"))
                .message(format!("No Template found with id: `{template_id}`"))
                .finish()
        })?;

    let command = TemplateCommand::DeleteTemplate {
        template_id: template_id.clone(),
    };

    command_handler(&template_id, &state.command.template, command).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
