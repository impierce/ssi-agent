use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{CredentialFormat, Display, HolderType, Status, Template, Visibility};
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

/// Data transfer object for Templates.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateDto {
    pub template_id: String,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub credential_format: Option<CredentialFormat>,
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
            title: value.title,
            display: value.display,
            credential_format: value.credential_format,
            creator: value.creator,
            holder_type: value.holder_type,
            modified_at: value.modified_at,
            tags: value.tags,
            status: value.status,
            visibility: value.visibility,
            description: value.description,
            r#type: value.r#type,
            schema: value.schema,
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PostTemplatesEndpointRequest {
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
    let template_id = uuid::Uuid::new_v4().to_string();

    let command = TemplateCommand::CreateTemplate {
        template_id: template_id.clone(),
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
            let filtered_templates: Vec<TemplateDto> = all_templates_view
                .templates
                .into_values()
                .filter(|_template|
                    // TODO: Apply filtering logic based on request parameters
                    true)
                .map(TemplateDto::from)
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
        .map(|template_view| (StatusCode::OK, Json(TemplateDto::from(template_view))).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use {Display, Template};

    #[test]
    fn test_template_response_serialization() {
        let template = Template {
            template_id: "test-id".to_string(),
            title: Some("Test Title".to_string()),
            display: Some(Display {
                name: "Test Display".to_string(),
                logo: None,
            }),
            credential_format: None,
            creator: None,
            holder_type: Some(HolderType::Individual),
            modified_at: None,
            tags: vec![],
            status: Default::default(),
            visibility: Default::default(),
            description: None,
            r#type: vec![],
            schema: None,
        };

        let response = TemplateDto::from(template);
        let json = serde_json::json!(&response);

        assert_eq!(json["templateId"], "test-id");
        assert_eq!(json["title"], "Test Title");
        assert_eq!(json["holderType"], "individual");
    }
}
