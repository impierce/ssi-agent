use crate::error::type_url;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::state::LibraryState;
pub use agent_library::template::aggregate::{CredentialFormat, Display, HolderType, Status, Visibility};
use agent_library::template::command::TemplateCommand;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Form, Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

#[derive(Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PostTemplatesEndpointRequest {
    pub source_template_id: Option<String>,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub credential_format: Option<CredentialFormat>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub tags: Vec<String>,
    pub status: Status,
    pub visibility: Visibility,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

#[axum_macros::debug_handler]
pub(crate) async fn post_templates(
    State(state): State<LibraryState>,
    Json(PostTemplatesEndpointRequest {
        source_template_id,
        title,
        display,
        credential_format,
        creator,
        holder_type,
        tags,
        status,
        visibility,
        description,
        r#type,
        schema,
    }): Json<PostTemplatesEndpointRequest>,
) -> Result<Response, ApiError> {
    // Handle template duplication if `source_template_id` field is provided.
    if let Some(old_template_id) = source_template_id {
        let new_template_id = Uuid::new_v4().to_string();

        // Fetch original template data.
        let original_template = query_handler(&old_template_id, &state.query.template)
            .await?
            .ok_or_else(|| {
                ApiError::builder(StatusCode::UNPROCESSABLE_ENTITY)
                    .title("Source Template Not Found")
                    .type_url(type_url("library#source-template-not-found"))
                    .message(format!("No Source Template found with id: `{old_template_id}`"))
                    .finish()
            })?;

        // TODO: Create a DuplicateTemplate command which takes care of duplication logic.
        // This would require defining a LibraryService that acts as a Domain Service.
        let create_command = TemplateCommand::CreateTemplate {
            template_id: new_template_id.clone(),
            source_template_id: Some(old_template_id),
            // Duplicate the template's original fields, appending "Copy" to the title.
            title: original_template.title.map(|t| format!("{} Copy", t)),
            display: original_template.display,
            credential_format: original_template.credential_format,
            creator: original_template.creator,
            holder_type: original_template.holder_type,
            tags: original_template.tags,
            status: Status::Draft,
            visibility: original_template.visibility,
            description: original_template.description,
            r#type: original_template.r#type,
            schema: original_template.schema,
        };

        command_handler(&new_template_id, &state.command.template, create_command).await?;

        // Return the duplicated template.
        let new_template = query_handler(&new_template_id, &state.query.template)
            .await?
            .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

        return Ok((
            StatusCode::CREATED,
            [(header::LOCATION, &format!("{API_VERSION}/templates/{new_template_id}"))],
            Json(new_template),
        )
            .into_response());
    }

    // If the `source_template_id` field is not provided, create a new template.
    let template_id = uuid::Uuid::new_v4().to_string();

    let command = TemplateCommand::CreateTemplate {
        template_id: template_id.clone(),
        source_template_id: None,
        title,
        display,
        credential_format,
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
                Json(template_view),
            )
                .into_response()
        })
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PatchTemplatesEndpointRequest {
    pub title: Option<String>,
    pub display: Option<Display>,
    pub credential_format: Option<CredentialFormat>,
    pub creator: Option<String>,
    pub holder_type: Option<HolderType>,
    pub tags: Vec<String>,
    pub status: Option<Status>,
    pub visibility: Option<Visibility>,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
}

#[axum_macros::debug_handler]
pub(crate) async fn patch_template(
    State(state): State<LibraryState>,
    Path(template_id): Path<String>,
    Json(PatchTemplatesEndpointRequest {
        title,
        display,
        credential_format,
        creator,
        holder_type,
        tags,
        status,
        visibility,
        description,
        r#type,
        schema,
    }): Json<PatchTemplatesEndpointRequest>,
) -> Result<Response, ApiError> {
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

    if let Some(credential_format) = credential_format {
        let command = TemplateCommand::UpdateCredentialFormat {
            template_id: template_id.clone(),
            credential_format,
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTemplatesEndpointRequest {
    // TODO: Add parameters for filtering templates
}

#[axum_macros::debug_handler]
pub(crate) async fn get_templates(
    State(state): State<LibraryState>,
    Form(GetTemplatesEndpointRequest {}): Form<GetTemplatesEndpointRequest>,
) -> Result<Response, ApiError> {
    debug!("Request Params - ");

    let filtered_templates = query_handler("all_templates", &state.query.all_templates)
        .await?
        .map(|all_templates_view| {
            let filtered_templates: Vec<_> = all_templates_view
                .templates
                .into_values()
                .filter(|template| {
                    template.status != Status::Deleted
                    // TODO: Apply filtering logic based on request parameters
                })
                .collect();

            filtered_templates
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_templates)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn get_template(
    State(state): State<LibraryState>,
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
        .map(|template_view| (StatusCode::OK, Json(template_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[axum_macros::debug_handler]
pub(crate) async fn delete_template(
    State(state): State<LibraryState>,
    Path(template_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = TemplateCommand::DeleteTemplate {
        template_id: template_id.clone(),
    };

    command_handler(&template_id, &state.command.template, command).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
