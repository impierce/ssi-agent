use crate::error::IntoApiErrorExt;
use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{
    DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Template, Visibility,
};
use agent_library::template::command::TemplateCommand;
use agent_library::template::error::TemplateError;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use uuid::Uuid;

pub mod openapi;

/// Data transfer object for Templates.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = Template)]
pub struct TemplateDto {
    #[serde(rename = "id")]
    pub template_id: String,
    pub title: String,
    pub display: Option<Display>,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub modified_at: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Status,
    pub visibility: Visibility,
    pub credential_expiration: Expiration,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub pre_authorized: bool,
}

impl From<Template> for TemplateDto {
    fn from(value: Template) -> Self {
        Self {
            template_id: value.template_id,
            title: value.title,
            display: value.display,
            data_model: value.data_model,
            holder_type: value.holder_type,
            modified_at: value.modified_at,
            tags: value.tags,
            status: value.status,
            visibility: value.visibility,
            credential_expiration: value.credential_expiration,
            description: value.description,
            r#type: value.r#type,
            schema: *value.schema,
            schema_properties_attributes: value.schema_properties_attributes,
            pre_authorized: value.pre_authorized,
        }
    }
}

fn default_pre_authorized() -> bool {
    false
}

#[derive(Debug, Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateNewTemplateRequestBody {
    pub title: String,
    pub display: Option<Display>,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub tags: Option<Vec<String>>,
    pub status: Status,
    pub visibility: Visibility,
    pub credential_expiration: Option<Expiration>,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    #[serde(default = "default_pre_authorized")]
    pub pre_authorized: bool,
}

/// Create a new template
///
/// Creates a new template which can be used to issue credentials.
#[utoipa::path(
    post,
    path = "/create-new-template",
    tags = ["Library", "Templates"],
    request_body(
        content = CreateNewTemplateRequestBody,
        examples(
            ("Standard template" = (
                description = "A simple example that will issue credentials in the W3C Verifiable Credentials Data Model v1.1 format.",
                value = json!({ "title": "Standard template", "dataModel": "w3c_vc_data_model_v1-1", "holderType": "individual" })
            )),
            ("OpenBadges template" = (
                description = "An OpenBadges 3.0 template. The fields `achievement.name`, `achievement.description`, and `achievement.criteria.narrative` must be explicitly included in the schema.",
                value = json!({ "title": "OpenBadges template", "dataModel": "open_badges_3-0", "holderType": "individual", "schema": { "type": "object", "properties": { "achievement.name": { "type": "string" }, "achievement.description": { "type": "string" }, "achievement.criteria.narrative": { "type": "string" } } } })
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
    Json(CreateNewTemplateRequestBody {
        title,
        display,
        data_model,
        holder_type,
        tags,
        status,
        visibility,
        credential_expiration,
        description,
        r#type,
        schema,
        schema_properties_attributes,
        pre_authorized,
    }): Json<CreateNewTemplateRequestBody>,
) -> Result<Response, ApiError> {
    let template_id = Uuid::new_v4().to_string();

    let command = TemplateCommand::CreateNewTemplate {
        template_id: template_id.clone(),
        source_template_id: None,
        title,
        display: Box::new(display),
        data_model,
        holder_type,
        tags,
        status,
        visibility,
        credential_expiration,
        description,
        r#type,
        schema: Box::new(schema),
        schema_properties_attributes: schema_properties_attributes
            .map(|attrs| attrs.into_iter().map(|(k, v)| (k, v.strip_non_removable())).collect()),
        pre_authorized,
    };

    command_handler(&template_id, &state.command.template, command).await?;

    // Return the template.
    query_handler(&template_id, &state.query.template)
        .await?
        .map(|template_view| {
            (
                StatusCode::CREATED,
                [(
                    header::LOCATION,
                    &format!("{API_VERSION}/get-template-by-id/{template_id}"),
                )],
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
    path = "/duplicate-template",
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
        .filter(|template| template.status != Status::Deleted)
        .ok_or_else(|| TemplateError::SourceTemplateNotFound(source_template_id.clone()).into_api_error())?;

    let command = TemplateCommand::CreateNewTemplate {
        template_id: new_template_id.clone(),
        source_template_id: Some(source_template_id),
        title: format!("{} Copy", original_template.title),
        display: Box::new(original_template.display),
        data_model: original_template.data_model,
        holder_type: original_template.holder_type,
        tags: original_template.tags,
        status: Status::Draft,
        visibility: Visibility::Private,
        credential_expiration: Some(original_template.credential_expiration),
        description: original_template.description,
        r#type: original_template.r#type,
        schema: original_template.schema,
        schema_properties_attributes: original_template.schema_properties_attributes,
        pre_authorized: original_template.pre_authorized,
    };

    command_handler(&new_template_id, &state.command.template, command).await?;

    // Return the duplicated template.
    let new_template = query_handler(&new_template_id, &state.query.template)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            &format!("{API_VERSION}/get-template-by-id/{new_template_id}"),
        )],
        Json(TemplateDto::from(new_template)),
    )
        .into_response())
}

#[derive(Debug, Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct UpdateTemplateEndpointRequest {
    #[serde(rename = "id")]
    pub template_id: String,
    pub title: Option<String>,
    pub display: Option<Display>,
    pub tags: Option<Vec<String>>,
    pub status: Option<Status>,
    pub visibility: Option<Visibility>,
    pub credential_expiration: Option<Expiration>,
    pub description: Option<String>,
    pub r#type: Option<Vec<String>>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub pre_authorized: Option<bool>,
}

/// Update a template
///
/// Updates an existing template with the provided content.
#[utoipa::path(
    post,
    path = "/update-template",
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
        tags,
        status,
        visibility,
        credential_expiration,
        description,
        r#type,
        schema,
        schema_properties_attributes,
        pre_authorized,
    }): Json<UpdateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_id.is_empty() {
        return Err(TemplateError::TemplateIdMissing.into_api_error());
    }

    query_handler(&template_id, &state.query.template)
        .await?
        .filter(|t| t.status != Status::Deleted)
        .ok_or_else(|| TemplateError::TemplateNotFound(template_id.clone()).into_api_error())?;

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

    if let Some(tags) = tags {
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

    if let Some(credential_expiration) = credential_expiration {
        let command = TemplateCommand::UpdateCredentialExpiration {
            template_id: template_id.clone(),
            credential_expiration,
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

    if let Some(r#type) = r#type {
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

    if let Some(schema_properties_attributes) = schema_properties_attributes {
        let command = TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id: template_id.clone(),
            schema_properties_attributes: schema_properties_attributes
                .into_iter()
                .map(|(k, v)| (k, v.strip_non_removable()))
                .collect(),
        };
        command_handler(&template_id, &state.command.template, command).await?;
    }

    if let Some(pre_authorized) = pre_authorized {
        let command = TemplateCommand::UpdatePreAuthorized {
            template_id: template_id.clone(),
            pre_authorized,
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
    path = "/list-all-templates",
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
            let mut filtered_templates: Vec<TemplateDto> = all_templates_view
                .templates
                .into_values()
                .filter(|template| {
                    template.status != Status::Deleted
                    // TODO: Apply filtering logic based on request parameters
                })
                .map(TemplateDto::from)
                .collect();

            // Sort by most recently modified first (RFC 3339 strings are lexicographically comparable).
            filtered_templates.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

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
    path = "/get-template-by-id/{id}",
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
    path = "/delete-template",
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
        return Err(TemplateError::TemplateIdMissing.into_api_error());
    }

    query_handler(&template_id, &state.query.template)
        .await?
        .filter(|t| t.status != Status::Deleted)
        .ok_or_else(|| TemplateError::TemplateNotFound(template_id.clone()).into_api_error())?;

    let command = TemplateCommand::DeleteTemplate {
        template_id: template_id.clone(),
    };

    command_handler(&template_id, &state.command.template, command).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_store::{in_memory::InMemory, library_state};
    use axum::{body::to_bytes, response::IntoResponse};
    use serde_json::json;
    use std::sync::Arc;

    async fn create_source_template(state: &Arc<LibraryState>, template_id: &str, visibility: Visibility) {
        create_source_template_with_title(state, template_id, "Source Template", visibility).await;
    }

    async fn create_source_template_with_title(
        state: &Arc<LibraryState>,
        template_id: &str,
        title: &str,
        visibility: Visibility,
    ) {
        command_handler(
            template_id,
            &state.command.template,
            TemplateCommand::CreateNewTemplate {
                template_id: template_id.to_string(),
                source_template_id: None,
                title: title.to_string(),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV1_1,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Draft,
                visibility,
                credential_expiration: Some(Expiration::Never),
                description: Some("Template description".to_string()),
                r#type: vec!["VerifiableCredential".to_string()],
                schema: Box::new(Some(json!({
                    "type": "object",
                    "properties": {
                        "first_name": { "type": "string" }
                    },
                    "required": ["first_name"]
                }))),
                schema_properties_attributes: None,
                pre_authorized: true,
            },
        )
        .await
        .unwrap();
    }

    #[test]
    fn create_template_request_accepts_credential_expiration() {
        let request = serde_json::from_value::<CreateNewTemplateRequestBody>(json!({
            "title": "Standard template",
            "dataModel": "w3c_vc_data_model_v1-1",
            "holderType": "individual",
            "credentialExpiration": { "type": "never" }
        }))
        .unwrap();

        assert_eq!(request.credential_expiration, Some(Expiration::Never));
    }

    #[test]
    fn create_template_request_rejects_legacy_fields() {
        let creator_error = serde_json::from_value::<CreateNewTemplateRequestBody>(json!({
            "title": "Standard template",
            "dataModel": "w3c_vc_data_model_v1-1",
            "holderType": "individual",
            "creator": "legacy"
        }))
        .err()
        .unwrap();
        assert!(creator_error.to_string().contains("creator"));

        let expiration_error = serde_json::from_value::<CreateNewTemplateRequestBody>(json!({
            "title": "Standard template",
            "dataModel": "w3c_vc_data_model_v1-1",
            "holderType": "individual",
            "expiration": { "type": "never" }
        }))
        .err()
        .unwrap();
        assert!(expiration_error.to_string().contains("expiration"));
    }

    #[test]
    fn update_template_request_accepts_credential_expiration() {
        let request = serde_json::from_value::<UpdateTemplateEndpointRequest>(json!({
            "id": "template-id",
            "credentialExpiration": { "type": "never" }
        }))
        .unwrap();

        assert_eq!(request.credential_expiration, Some(Expiration::Never));
    }

    #[test]
    fn update_template_request_rejects_legacy_fields() {
        let creator_error = serde_json::from_value::<UpdateTemplateEndpointRequest>(json!({
            "id": "template-id",
            "creator": "legacy"
        }))
        .err()
        .unwrap();
        assert!(creator_error.to_string().contains("creator"));

        let expiration_error = serde_json::from_value::<UpdateTemplateEndpointRequest>(json!({
            "id": "template-id",
            "expiration": { "type": "never" }
        }))
        .err()
        .unwrap();
        assert!(expiration_error.to_string().contains("expiration"));
    }

    #[test]
    fn template_dto_hides_internal_source_template_id() {
        let dto = TemplateDto::from(Template {
            template_id: "template-id".to_string(),
            source_template_id: Some("parent-template-id".to_string()),
            title: "Template".to_string(),
            display: None,
            data_model: DataModel::W3CVcDataModelV1_1,
            holder_type: HolderType::Individual,
            modified_at: Some("2024-01-01T00:00:00Z".to_string()),
            tags: None,
            status: Status::Draft,
            visibility: Visibility::Private,
            credential_expiration: Expiration::Never,
            description: None,
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(None),
            schema_properties_attributes: None,
            pre_authorized: false,
        });

        let serialized = serde_json::to_value(dto).unwrap();

        assert!(serialized.get("sourceTemplateId").is_none());
    }

    #[tokio::test]
    async fn duplicate_template_resets_visibility_and_hides_lineage() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template(&state, "source-template", Visibility::Public).await;

        let response = duplicate_template(
            State(state),
            Json(DuplicateTemplateEndpointRequest {
                source_template_id: "source-template".to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["status"], "draft");
        assert_eq!(body["visibility"], "private");
        assert_eq!(body["title"], "Source Template Copy");
        assert!(body.get("sourceTemplateId").is_none());
    }

    #[tokio::test]
    async fn duplicate_template_rejects_deleted_source_template() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template(&state, "deleted-source", Visibility::Private).await;

        command_handler(
            "deleted-source",
            &state.command.template,
            TemplateCommand::DeleteTemplate {
                template_id: "deleted-source".to_string(),
            },
        )
        .await
        .unwrap();

        let response = duplicate_template(
            State(state),
            Json(DuplicateTemplateEndpointRequest {
                source_template_id: "deleted-source".to_string(),
            }),
        )
        .await
        .unwrap_err()
        .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["title"], "Source Template Not Found");
    }

    #[tokio::test]
    async fn create_template_returns_created_template() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);

        let response = create_template(
            State(state),
            Json(CreateNewTemplateRequestBody {
                title: "Created Template".to_string(),
                display: None,
                data_model: DataModel::W3CVcDataModelV1_1,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Draft,
                visibility: Visibility::Private,
                credential_expiration: Some(Expiration::Never),
                description: Some("Created description".to_string()),
                r#type: vec!["EmployeeCredential".to_string()],
                schema: None,
                schema_properties_attributes: None,
                pre_authorized: true,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["title"], "Created Template");
        assert_eq!(body["description"], "Created description");
        assert_eq!(body["type"], json!(["VerifiableCredential", "EmployeeCredential"]));
    }

    #[tokio::test]
    async fn update_template_requires_id() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);

        let response = update_template(
            State(state),
            Json(UpdateTemplateEndpointRequest {
                template_id: String::new(),
                title: Some("Updated title".to_string()),
                display: None,
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: None,
                description: None,
                r#type: None,
                schema: None,
                schema_properties_attributes: None,
                pre_authorized: None,
            }),
        )
        .await
        .unwrap_err()
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["title"], "Template ID Missing");
    }

    #[tokio::test]
    async fn update_template_rejects_deleted_template() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template(&state, "deleted-template", Visibility::Private).await;

        command_handler(
            "deleted-template",
            &state.command.template,
            TemplateCommand::DeleteTemplate {
                template_id: "deleted-template".to_string(),
            },
        )
        .await
        .unwrap();

        let response = update_template(
            State(state),
            Json(UpdateTemplateEndpointRequest {
                template_id: "deleted-template".to_string(),
                title: Some("Updated title".to_string()),
                display: None,
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: None,
                description: None,
                r#type: None,
                schema: None,
                schema_properties_attributes: None,
                pre_authorized: None,
            }),
        )
        .await
        .unwrap_err()
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["title"], "Template Not Found");
    }

    #[tokio::test]
    async fn update_template_applies_type_and_credential_expiration_changes() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template(&state, "template-to-update", Visibility::Private).await;

        let response = update_template(
            State(state.clone()),
            Json(UpdateTemplateEndpointRequest {
                template_id: "template-to-update".to_string(),
                title: None,
                display: None,
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: Some(Expiration::Duration("P30D".to_string())),
                description: Some("Updated description".to_string()),
                r#type: Some(vec!["EmployeeCredential".to_string()]),
                schema: None,
                schema_properties_attributes: None,
                pre_authorized: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(template.credential_expiration, Expiration::Duration("P30D".to_string()));
        assert_eq!(template.description.as_deref(), Some("Updated description"));
        assert_eq!(
            template.r#type,
            vec!["VerifiableCredential".to_string(), "EmployeeCredential".to_string(),]
        );
    }

    #[tokio::test]
    async fn get_templates_filters_deleted_and_sorts_latest_first() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template_with_title(&state, "older-template", "Older Template", Visibility::Private).await;
        create_source_template_with_title(&state, "newer-template", "Newer Template", Visibility::Private).await;
        create_source_template_with_title(&state, "deleted-template", "Deleted Template", Visibility::Private).await;

        command_handler(
            "deleted-template",
            &state.command.template,
            TemplateCommand::DeleteTemplate {
                template_id: "deleted-template".to_string(),
            },
        )
        .await
        .unwrap();

        let response = get_templates(State(state)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let titles: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|template| template["title"].as_str().unwrap())
            .collect();

        assert_eq!(titles, vec!["Newer Template", "Older Template"]);
    }

    #[tokio::test]
    async fn delete_template_hides_template_from_get_endpoint() {
        let state = Arc::new(library_state(&InMemory, Default::default(), Default::default()).await);
        create_source_template(&state, "template-to-delete", Visibility::Private).await;

        let response = delete_template(
            State(state.clone()),
            Json(DeleteTemplateEndpointRequest {
                template_id: "template-to-delete".to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = get_template(State(state), Path("template-to-delete".to_string()))
            .await
            .unwrap_err()
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
