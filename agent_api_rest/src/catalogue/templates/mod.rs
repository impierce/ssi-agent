use crate::handlers::{command_handler, query_handler};
use crate::API_VERSION;
use agent_catalogue::state::CatalogueState;
use agent_catalogue::template::command::TemplateCommand;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Form, Json,
};
use http_api_problem::ApiError;
use hyper::{header, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostTemplatesEndpointRequest {
    // TODO: Define template fields
}

#[axum_macros::debug_handler]
pub(crate) async fn post_templates(
    State(state): State<CatalogueState>,
    Json(PostTemplatesEndpointRequest {}): Json<PostTemplatesEndpointRequest>,
) -> Result<Response, ApiError> {
    let template_id = uuid::Uuid::new_v4().to_string();

    let command = TemplateCommand::CreateTemplate {
        template_id: template_id.clone(),
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetTemplatesEndpointRequest {
    // TODO: Add parameters for filtering templates
}

#[axum_macros::debug_handler]
pub(crate) async fn get_templates(
    State(state): State<CatalogueState>,
    Form(GetTemplatesEndpointRequest {}): Form<GetTemplatesEndpointRequest>,
) -> Result<Response, ApiError> {
    debug!("Request Params - ");

    let filtered_templates = query_handler("all_templates", &state.query.all_templates)
        .await?
        .map(|all_templates_view| {
            let filtered_templates: Vec<_> = all_templates_view
                .templates
                .into_values()
                .filter(|_template| 
                    // TODO: Apply filtering logic based on request parameters
                    true)
                .collect();

            filtered_templates
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(filtered_templates)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn get_template(
    State(state): State<CatalogueState>,
    Path(template_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&template_id, &state.query.template)
        .await?
        .map(|template_view| (StatusCode::OK, Json(template_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
}
