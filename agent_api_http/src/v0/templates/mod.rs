use crate::error::IntoApiErrorExt;
use crate::extractors::RequestActor;
use crate::handlers::{caller, command_handler, query_handler};
use crate::API_VERSION;
use agent_library::queries;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{
    DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Template, Visibility,
};
use agent_library::template::command::TemplateCommand;
use agent_library::template::error::TemplateError;
use agent_library::template::origin::TemplateOrigin;
use agent_shared::config::Authorization;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Caller;
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
    pub display: Display,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub modified_at: Option<String>,
    pub tags: Option<Vec<String>>,
    #[schema(inline)]
    pub status: Status,
    pub visibility: Visibility,
    pub credential_expiration: Expiration,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub holder_authorization: Authorization,
    /// Where the template was imported from, if it was.
    pub origin: Option<TemplateOrigin>,
}

impl From<Template> for TemplateDto {
    fn from(value: Template) -> Self {
        // An empty `display.name` — or no stored `display` at all — means no explicit display name
        // has been set: fall back to the current title so the API always surfaces a usable name,
        // without baking the title into the stored template (which would stop later title updates
        // from being reflected here).
        let display = Display::resolve(value.display, &value.title);

        Self {
            template_id: value.template_id,
            title: value.title,
            display,
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
            holder_authorization: value.holder_authorization,
            origin: value.origin,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateNewTemplateRequestBody {
    pub title: String,
    pub display: Option<Display>,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub tags: Option<Vec<String>>,
    #[schema(schema_with = status_schema)]
    pub status: Status,
    pub visibility: Visibility,
    pub credential_expiration: Option<Expiration>,
    pub description: Option<String>,
    pub r#type: Vec<String>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub holder_authorization: Authorization,
    /// Where the template is imported from, if it is.
    pub origin: Option<TemplateOrigin>,
}

fn status_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    let mut schema = <Status as utoipa::PartialSchema>::schema();
    if let utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(object)) = &mut schema {
        object.default = serde_json::to_value(Status::default()).ok();
    }
    schema
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
                value = json!({ "title": "OpenBadges template", "dataModel": "open_badges_3-0", "holderType": "individual", "schema": { "type": "object", "properties": { "achievement": { "type": "object", "properties": { "name": { "type": "string" }, "description": { "type": "string" }, "criteria": { "type": "object", "properties": { "narrative": { "type": "string" } } } } } } } })
            ))
        )
    ),
    responses(
        (status = 201, description = "New template created successfully", headers(("Location", description = "The path of the newly created template")), body = TemplateDto),
        (status = 400, description = "Malformed JSON request body, or an invalid template definition"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn create_template(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
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
        holder_authorization,
        origin,
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
        holder_authorization,
        origin: origin.map(Box::new),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &template_id,
        &state.command.template,
        command,
    )
    .await?;

    // Return the template.
    queries::get_template(&state, Caller::Internal, &template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
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
        (status = 400, description = "Malformed JSON request body"),
        (status = 422, description = "Source template not found, or the request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn duplicate_template(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(DuplicateTemplateEndpointRequest { source_template_id }): Json<DuplicateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    let new_template_id = Uuid::new_v4().to_string();

    let original_template = queries::get_template(&state, caller(actor.clone()), &source_template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
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
        holder_authorization: original_template.holder_authorization,
        // A duplicate is a local copy, not an import.
        origin: None,
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &new_template_id,
        &state.command.template,
        command,
    )
    .await?;

    // Return the duplicated template.
    let new_template = queries::get_template(&state, Caller::Internal, &new_template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
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
    #[schema(inline)]
    pub status: Option<Status>,
    pub visibility: Option<Visibility>,
    pub credential_expiration: Option<Expiration>,
    pub description: Option<String>,
    pub r#type: Option<Vec<String>>,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub holder_authorization: Option<Authorization>,
    /// Replaces the origin; `null` removes it. Applied after all other fields.
    #[serde(with = "::serde_with::rust::double_option", skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<TemplateOrigin>)]
    pub origin: Option<Option<TemplateOrigin>>,
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
        (status = 204, description = "Template updated successfully"),
        (status = 400, description = "Malformed JSON request body, or a missing template ID"),
        (status = 404, description = "Template not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn update_template(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
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
        holder_authorization,
        origin,
    }): Json<UpdateTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_id.is_empty() {
        return Err(TemplateError::TemplateIdMissing.into_api_error());
    }

    queries::get_template(&state, caller(actor.clone()), &template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| TemplateError::TemplateNotFound(template_id.clone()).into_api_error())?;

    if let Some(title) = title {
        let command = TemplateCommand::UpdateTitle {
            template_id: template_id.clone(),
            title,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(display) = display {
        let command = TemplateCommand::UpdateDisplay {
            template_id: template_id.clone(),
            display,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(tags) = tags {
        let command = TemplateCommand::UpdateTags {
            template_id: template_id.clone(),
            tags,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(status) = status {
        let command = TemplateCommand::UpdateStatus {
            template_id: template_id.clone(),
            status,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(visibility) = visibility {
        let command = TemplateCommand::UpdateVisibility {
            template_id: template_id.clone(),
            visibility,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(credential_expiration) = credential_expiration {
        let command = TemplateCommand::UpdateCredentialExpiration {
            template_id: template_id.clone(),
            credential_expiration,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(description) = description {
        let command = TemplateCommand::UpdateDescription {
            template_id: template_id.clone(),
            description,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(r#type) = r#type {
        let command = TemplateCommand::UpdateType {
            template_id: template_id.clone(),
            r#type,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(schema) = schema {
        let command = TemplateCommand::UpdateSchema {
            template_id: template_id.clone(),
            schema,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(schema_properties_attributes) = schema_properties_attributes {
        let command = TemplateCommand::UpdateSchemaPropertiesAttributes {
            template_id: template_id.clone(),
            schema_properties_attributes: schema_properties_attributes
                .into_iter()
                .map(|(k, v)| (k, v.strip_non_removable()))
                .collect(),
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    if let Some(holder_authorization) = holder_authorization {
        let command = TemplateCommand::UpdateHolderAuthorization {
            template_id: template_id.clone(),
            holder_authorization,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
    }

    // Last: if an earlier update fails, the previous origin digest no longer matches the content.
    if let Some(origin) = origin {
        let command = TemplateCommand::UpdateOrigin {
            template_id: template_id.clone(),
            origin,
        };
        command_handler(
            state.authorization_checker.clone(),
            actor.clone(),
            &template_id,
            &state.command.template,
            command,
        )
        .await?;
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
pub(crate) async fn get_templates(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
) -> Result<Response, ApiError> {
    let filtered_templates = query_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        "all_templates",
        None,
        &state.query.all_templates,
    )
    .await?
    .map(|all_templates_view| {
        let mut filtered_templates: Vec<TemplateDto> = all_templates_view
            .templates
            .into_values()
            // TODO: Apply filtering logic based on request parameters
            .map(TemplateDto::from)
            .collect();

        filtered_templates.sort_by(|left, right| {
            crate::utils::compare_rfc3339_newest_first(
                left.modified_at.as_deref(),
                &left.template_id,
                right.modified_at.as_deref(),
                &right.template_id,
            )
        });

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
        (status = 200, description = "Template retrieved successfully", body = TemplateDto),
        (status = 400, description = "Invalid path parameter"),
        (status = 404, description = "Template not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn get_template(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Path(template_id): Path<String>,
) -> Result<Response, ApiError> {
    queries::get_template(&state, caller(actor), &template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
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
        (status = 204, description = "Template deleted successfully"),
        (status = 400, description = "Malformed JSON request body, or a missing template ID"),
        (status = 404, description = "Template not found"),
        (status = 422, description = "Request body does not match the expected schema"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn delete_template(
    State(state): State<Arc<LibraryState>>,
    RequestActor(actor): RequestActor,
    Json(DeleteTemplateEndpointRequest { template_id }): Json<DeleteTemplateEndpointRequest>,
) -> Result<Response, ApiError> {
    if template_id.is_empty() {
        return Err(TemplateError::TemplateIdMissing.into_api_error());
    }

    queries::get_template(&state, caller(actor.clone()), &template_id)
        .await
        .map_err(IntoApiErrorExt::into_api_error)?
        .ok_or_else(|| TemplateError::TemplateNotFound(template_id.clone()).into_api_error())?;

    let command = TemplateCommand::DeleteTemplate {
        template_id: template_id.clone(),
    };

    command_handler(
        state.authorization_checker.clone(),
        actor.clone(),
        &template_id,
        &state.command.template,
        command,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::{public_command_handler as command_handler, public_query_handler as query_handler};
    use agent_library::library_state;
    use agent_store::in_memory::InMemory;
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
                holder_authorization: Authorization::default(),
                origin: None,
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
    fn template_dto_always_carries_a_display_object() {
        // Wallets and verifiers render a name (and logo) from `display` unconditionally, so a
        // template that never stored one still gets an object derived from its title.
        let dto = TemplateDto::from(Template {
            template_id: "template-id".to_string(),
            source_template_id: None,
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
            holder_authorization: Authorization::default(),
            origin: None,
        });

        assert_eq!(dto.display.name, "Template");
        assert!(dto.display.logo.is_none());

        let serialized = serde_json::to_value(dto).unwrap();
        assert_eq!(serialized["display"]["name"], "Template");
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
            holder_authorization: Authorization::default(),
            origin: None,
        });

        let serialized = serde_json::to_value(dto).unwrap();

        assert!(serialized.get("sourceTemplateId").is_none());
    }

    #[tokio::test]
    async fn duplicate_template_resets_visibility_and_hides_lineage() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        create_source_template(&state, "source-template", Visibility::Public).await;

        let response = duplicate_template(
            State(state),
            RequestActor(None),
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
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
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
            RequestActor(None),
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

    #[test]
    fn create_template_status_schema_is_a_string_enum_with_a_default() {
        let schema = <CreateNewTemplateRequestBody as utoipa::PartialSchema>::schema();
        let schema = serde_json::to_value(schema).unwrap();

        assert_eq!(
            schema["properties"]["status"],
            json!({
                "type": "string",
                "enum": ["draft", "published", "archived", "deleted"],
                "default": "draft"
            })
        );
    }

    #[tokio::test]
    async fn create_template_returns_created_template() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let response = create_template(
            State(state),
            RequestActor(None),
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
                holder_authorization: Authorization::default(),
                origin: None,
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
    async fn create_template_defaults_empty_display_name_to_title() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let response = create_template(
            State(state),
            RequestActor(None),
            Json(CreateNewTemplateRequestBody {
                title: "Created Template".to_string(),
                display: Some(Display {
                    name: String::new(),
                    logo: None,
                }),
                data_model: DataModel::W3CVcDataModelV1_1,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Draft,
                visibility: Visibility::Private,
                credential_expiration: Some(Expiration::Never),
                description: None,
                r#type: vec![],
                schema: None,
                schema_properties_attributes: None,
                holder_authorization: Authorization::default(),
                origin: None,
            }),
        )
        .await
        .unwrap();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["display"]["name"], "Created Template");
    }

    #[tokio::test]
    async fn update_template_requires_id() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let response = update_template(
            State(state),
            RequestActor(None),
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
                holder_authorization: None,
                origin: None,
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
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
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
            RequestActor(None),
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
                holder_authorization: None,
                origin: None,
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
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        create_source_template(&state, "template-to-update", Visibility::Private).await;

        let response = update_template(
            State(state.clone()),
            RequestActor(None),
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
                holder_authorization: None,
                origin: None,
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
    async fn update_template_keeps_empty_display_name_and_resolves_it_on_read() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        create_source_template(&state, "template-to-update", Visibility::Private).await;

        let response = update_template(
            State(state.clone()),
            RequestActor(None),
            Json(UpdateTemplateEndpointRequest {
                template_id: "template-to-update".to_string(),
                title: Some("Updated title".to_string()),
                display: Some(Display {
                    name: String::new(),
                    logo: None,
                }),
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: None,
                description: None,
                r#type: None,
                schema: None,
                schema_properties_attributes: None,
                holder_authorization: None,
                origin: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(template.display.as_ref().unwrap().name, "");
        assert_eq!(TemplateDto::from(template).display.name, "Updated title");

        update_template(
            State(state.clone()),
            RequestActor(None),
            Json(UpdateTemplateEndpointRequest {
                template_id: "template-to-update".to_string(),
                title: Some("Renamed title".to_string()),
                display: None,
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: None,
                description: None,
                r#type: None,
                schema: None,
                schema_properties_attributes: None,
                holder_authorization: None,
                origin: None,
            }),
        )
        .await
        .unwrap();

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(TemplateDto::from(template).display.name, "Renamed title");
    }

    #[tokio::test]
    async fn update_template_keeps_title_and_display_name_independent() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        create_source_template(&state, "template-to-update", Visibility::Private).await;

        update_template(
            State(state.clone()),
            RequestActor(None),
            Json(UpdateTemplateEndpointRequest {
                template_id: "template-to-update".to_string(),
                title: None,
                display: Some(Display {
                    name: "Custom display name".to_string(),
                    logo: None,
                }),
                tags: None,
                status: None,
                visibility: None,
                credential_expiration: None,
                description: None,
                r#type: None,
                schema: None,
                schema_properties_attributes: None,
                holder_authorization: None,
                origin: None,
            }),
        )
        .await
        .unwrap();

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(template.title, "Source Template");
        assert_eq!(template.display.unwrap().name, "Custom display name");

        update_template(
            State(state.clone()),
            RequestActor(None),
            Json(UpdateTemplateEndpointRequest {
                template_id: "template-to-update".to_string(),
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
                holder_authorization: None,
                origin: None,
            }),
        )
        .await
        .unwrap();

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(template.title, "Updated title");
        assert_eq!(template.display.unwrap().name, "Custom display name");
    }

    #[tokio::test]
    async fn get_templates_filters_deleted_and_sorts_latest_first() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
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

        let response = get_templates(State(state), RequestActor(None)).await.unwrap();
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
    async fn update_template_applies_tags_visibility_schema_and_holder_authorization() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        // W3C VC 1.1 templates cannot have schema properties attributes.
        command_handler(
            "template-to-update",
            &state.command.template,
            TemplateCommand::CreateNewTemplate {
                template_id: "template-to-update".to_string(),
                source_template_id: None,
                title: "Template".to_string(),
                display: Box::new(None),
                data_model: DataModel::W3CVcDataModelV2_0,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Draft,
                visibility: Visibility::Private,
                credential_expiration: Some(Expiration::Never),
                description: None,
                r#type: vec!["VerifiableCredential".to_string()],
                schema: Box::new(None),
                schema_properties_attributes: None,
                holder_authorization: Authorization::default(),
                origin: None,
            },
        )
        .await
        .unwrap();
        let update = |request: UpdateTemplateEndpointRequest| {
            update_template(
                State(state.clone()),
                RequestActor(None),
                Json(UpdateTemplateEndpointRequest {
                    template_id: "template-to-update".to_string(),
                    ..request
                }),
            )
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "first_name": { "type": "string" },
                "last_name": { "type": "string" }
            },
            "required": ["first_name", "last_name"]
        });
        let schema_properties_attributes = HashMap::from([(
            "/last_name".to_string(),
            PropertyAttribute {
                selectively_disclosable: true,
                non_removable: false,
                r#type: None,
            },
        )]);
        let holder_authorization = Authorization {
            pre_authorized: true,
            tx_code_constraints: None,
        };
        let response = update(UpdateTemplateEndpointRequest {
            tags: Some(vec!["education".to_string(), "diploma".to_string()]),
            schema: Some(schema.clone()),
            schema_properties_attributes: Some(schema_properties_attributes.clone()),
            holder_authorization: Some(holder_authorization.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            template.tags,
            Some(vec!["education".to_string(), "diploma".to_string()])
        );
        assert_eq!(*template.schema, Some(schema));
        assert_eq!(
            template.schema_properties_attributes,
            Some(schema_properties_attributes)
        );
        assert_eq!(template.holder_authorization, holder_authorization);

        // A draft cannot be made public, but a published template can.
        let response = update(UpdateTemplateEndpointRequest {
            visibility: Some(Visibility::Public),
            ..Default::default()
        })
        .await
        .unwrap_err()
        .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let response = update(UpdateTemplateEndpointRequest {
            status: Some(Status::Published),
            visibility: Some(Visibility::Public),
            ..Default::default()
        })
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let template = query_handler("template-to-update", &state.query.template)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(template.status, Status::Published);
        assert_eq!(template.visibility, Visibility::Public);

        let response = delete_template(
            State(state.clone()),
            RequestActor(None),
            Json(DeleteTemplateEndpointRequest {
                template_id: String::new(),
            }),
        )
        .await
        .unwrap_err()
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_template_hides_template_from_get_endpoint() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        create_source_template(&state, "template-to-delete", Visibility::Private).await;

        let response = delete_template(
            State(state.clone()),
            RequestActor(None),
            Json(DeleteTemplateEndpointRequest {
                template_id: "template-to-delete".to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = get_template(State(state), RequestActor(None), Path("template-to-delete".to_string()))
            .await
            .unwrap_err()
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn schema_properties_attributes_type_survives_api_round_trip() {
        // Verifies that `schemaPropertiesAttributes[*].type` (a caller-supplied rendering hint)
        // is preserved end-to-end: create template → retrieve via get endpoint → field still present.
        use agent_library::template::aggregate::FormFieldType;

        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let response = create_template(
            State(state.clone()),
            RequestActor(None),
            Json(CreateNewTemplateRequestBody {
                title: "Country Template".to_string(),
                display: None,
                data_model: DataModel::W3CVcDataModelV2_0,
                holder_type: HolderType::Individual,
                tags: None,
                status: Status::Draft,
                visibility: Visibility::Private,
                credential_expiration: None,
                description: None,
                r#type: vec![],
                schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "nationality": { "type": "string" }
                    }
                })),
                schema_properties_attributes: Some(HashMap::from([(
                    "/nationality".to_string(),
                    PropertyAttribute {
                        selectively_disclosable: false,
                        non_removable: false,
                        r#type: Some(FormFieldType::Country),
                    },
                )])),
                holder_authorization: Authorization::default(),
                origin: None,
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let template_id = created["id"].as_str().unwrap().to_string();

        let response = get_template(State(state), RequestActor(None), Path(template_id))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let retrieved: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(
            retrieved["schemaPropertiesAttributes"]["/nationality"]["type"],
            json!("country")
        );
    }

    const ORIGIN_SOURCE: &str = "https://issuer.example";

    fn origin_json(digest: &str) -> serde_json::Value {
        json!({ "source": ORIGIN_SOURCE, "sourceTemplateId": "0b7c", "digest": digest })
    }

    fn digest_of(byte: char) -> String {
        format!("sha256-{}", byte.to_string().repeat(64))
    }

    async fn create_from_json(state: &Arc<LibraryState>, body: serde_json::Value) -> serde_json::Value {
        let response = create_template(
            State(state.clone()),
            RequestActor(None),
            Json(serde_json::from_value(body).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    async fn update_from_json(state: &Arc<LibraryState>, body: serde_json::Value) -> Result<Response, ApiError> {
        update_template(
            State(state.clone()),
            RequestActor(None),
            Json(serde_json::from_value(body).unwrap()),
        )
        .await
    }

    async fn stored_template(state: &Arc<LibraryState>, template_id: &str) -> Template {
        queries::get_template(state, Caller::Internal, template_id)
            .await
            .unwrap()
            .unwrap()
    }

    #[test]
    fn update_template_request_tells_absent_origin_from_null_origin() {
        let parse = |body| {
            serde_json::from_value::<UpdateTemplateEndpointRequest>(body)
                .unwrap()
                .origin
        };

        assert_eq!(parse(json!({ "id": "template-id" })), None);
        assert_eq!(parse(json!({ "id": "template-id", "origin": null })), Some(None));
        assert_eq!(
            parse(json!({ "id": "template-id", "origin": origin_json(&digest_of('a')) })),
            Some(Some(serde_json::from_value(origin_json(&digest_of('a'))).unwrap()))
        );
    }

    #[tokio::test]
    async fn create_template_stores_and_returns_origin() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let created = create_from_json(
            &state,
            json!({ "title": "Imported", "origin": origin_json(&digest_of('a')) }),
        )
        .await;

        let mut stored = origin_json(&digest_of('a'));
        stored["checkForUpdates"] = json!(true);
        assert_eq!(
            created["origin"], stored,
            "an origin given without `checkForUpdates` checks for updates"
        );
    }

    #[tokio::test]
    async fn create_template_without_origin_returns_null_origin() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let created = create_from_json(&state, json!({ "title": "Local" })).await;

        assert_eq!(created["origin"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn create_template_rejects_invalid_origin() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);

        let response = create_template(
            State(state),
            RequestActor(None),
            Json(
                serde_json::from_value(json!({ "title": "Imported", "origin": origin_json("sha256-not-hex") }))
                    .unwrap(),
            ),
        )
        .await
        .unwrap_err()
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(body["title"], "Invalid Template Origin");
    }

    #[tokio::test]
    async fn update_template_replaces_keeps_and_removes_origin() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        let created = create_from_json(
            &state,
            json!({ "title": "Imported", "origin": origin_json(&digest_of('a')) }),
        )
        .await;
        let template_id = created["id"].as_str().unwrap();

        update_from_json(
            &state,
            json!({ "id": template_id, "title": "Re-imported", "origin": origin_json(&digest_of('b')) }),
        )
        .await
        .unwrap();
        let template = stored_template(&state, template_id).await;
        assert_eq!(template.title, "Re-imported");
        assert_eq!(template.origin.unwrap().digest, digest_of('b'));

        update_from_json(&state, json!({ "id": template_id, "title": "Edited" }))
            .await
            .unwrap();
        assert_eq!(
            stored_template(&state, template_id).await.origin.unwrap().digest,
            digest_of('b')
        );

        update_from_json(&state, json!({ "id": template_id, "origin": null }))
            .await
            .unwrap();
        assert_eq!(stored_template(&state, template_id).await.origin, None);
    }

    #[tokio::test]
    async fn update_template_applies_origin_after_other_fields() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        let created = create_from_json(
            &state,
            json!({ "title": "Imported", "origin": origin_json(&digest_of('a')) }),
        )
        .await;
        let template_id = created["id"].as_str().unwrap();

        // The title update fails, so the new origin is never stored and the old digest remains.
        update_from_json(
            &state,
            json!({ "id": template_id, "title": " ", "origin": origin_json(&digest_of('b')) }),
        )
        .await
        .unwrap_err();

        assert_eq!(
            stored_template(&state, template_id).await.origin.unwrap().digest,
            digest_of('a')
        );
    }

    #[tokio::test]
    async fn duplicate_template_does_not_copy_origin() {
        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        let created = create_from_json(
            &state,
            json!({ "title": "Imported", "origin": origin_json(&digest_of('a')) }),
        )
        .await;

        let response = duplicate_template(
            State(state),
            RequestActor(None),
            Json(DuplicateTemplateEndpointRequest {
                source_template_id: created["id"].as_str().unwrap().to_string(),
            }),
        )
        .await
        .unwrap();
        let duplicate: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();

        assert_eq!(duplicate["origin"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn origin_digest_detects_local_edits_of_an_imported_template() {
        use agent_library::template::public_content::PublicTemplateContent;

        // Public projection entry of a template at the source.
        let source_entry = json!({
            "title": "Diploma",
            "description": "Awarded on graduation",
            "display": { "name": "Diploma" },
            "dataModel": "w3c_vc_data_model_v2-0",
            "holderType": "individual",
            "type": ["VerifiableCredential"],
            "tags": ["education"],
            "credentialExpiration": { "type": "never" },
            "schema": { "type": "object", "properties": { "name": { "type": "string" } } },
            "schemaPropertiesAttributes": null,
            "modifiedAt": "2026-01-01T00:00:00Z",
        });
        let source_content: PublicTemplateContent = serde_json::from_value(source_entry.clone()).unwrap();
        let source_digest = source_content.digest();

        let state = Arc::new(library_state(&InMemory, &Default::default(), vec![]).await);
        let mut body = source_entry;
        let members = body.as_object_mut().unwrap();
        members.remove("modifiedAt");
        members.insert("origin".to_string(), origin_json(&source_digest));
        let created = create_from_json(&state, body).await;
        let template_id = created["id"].as_str().unwrap();

        let imported = stored_template(&state, template_id).await;
        assert_eq!(imported.public_content().digest(), imported.origin.unwrap().digest);

        update_from_json(&state, json!({ "id": template_id, "description": "Edited locally" }))
            .await
            .unwrap();

        let edited = stored_template(&state, template_id).await;
        assert_ne!(edited.public_content().digest(), edited.origin.unwrap().digest);
    }
}
