use crate::handlers::public_query_handler;
use agent_library::state::LibraryState;
use agent_library::template::aggregate::{
    DataModel, Display, Expiration, HolderType, PropertyAttribute, Status, Visibility,
};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

// Compared to the internal `TemplateDto` this omits the identifiers (`id` and `sourceTemplateId`)
// and `holderAuthorization` (issuance-side configuration). `status` and `visibility` are omitted as
// well: every template returned here is by definition published and public, so both fields would be
// constants. `modifiedAt` is kept because, absent a versioning system, it is the only signal a
// reader has of how fresh a template is.
//
// Data transfer object for publicly available Templates.
//
// A reduced projection of a template, carrying only what is needed to understand and render it.
// Templates carry no public identifier: a reader that imports one assigns its own, and can refer
// to an entry by its position in the returned array in the meantime.
#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = PublicTemplate)]
pub struct PublicTemplateDto {
    pub title: String,
    pub description: Option<String>,
    pub display: Option<Display>,
    pub data_model: DataModel,
    pub holder_type: HolderType,
    pub r#type: Vec<String>,
    pub tags: Option<Vec<String>>,
    pub credential_expiration: Expiration,
    pub schema: Option<serde_json::Value>,
    pub schema_properties_attributes: Option<HashMap<String, PropertyAttribute>>,
    pub modified_at: Option<String>,
}

impl From<agent_library::template::views::TemplateView> for PublicTemplateDto {
    fn from(value: agent_library::template::views::TemplateView) -> Self {
        Self {
            title: value.title,
            description: value.description,
            display: value.display,
            data_model: value.data_model,
            holder_type: value.holder_type,
            r#type: value.r#type,
            tags: value.tags,
            credential_expiration: value.credential_expiration,
            schema: *value.schema,
            schema_properties_attributes: value.schema_properties_attributes,
            modified_at: value.modified_at,
        }
    }
}

/// List all public templates
///
/// Lists every template that is publicly visible and published. No authentication is required.
#[utoipa::path(
    get,
    path = "/templates",
    operation_id = "get_public_templates",
    tags = ["Public"],
    responses(
        (status = 200, description = "All public templates retrieved successfully", body = [PublicTemplateDto])
    )
)]
// TODO: Read a dedicated `PublicTemplatesView` here once in-memory views built via event replay
// exist as a general facility. Deriving from `all_templates` makes every request deserialize the
// full template set — private and draft ones included — just to discard most of it. See
// `docs/adr/0004-public-templates-view-derived-at-query-time.md` for why that refactor was deferred
// and what it entails.
#[axum_macros::debug_handler]
pub(crate) async fn get_public_templates(State(state): State<Arc<LibraryState>>) -> Result<Response, ApiError> {
    let public_templates = public_query_handler("all_templates", &state.query.all_templates)
        .await?
        .map(|all_templates_view| {
            let mut public_templates: Vec<_> = all_templates_view
                .templates
                .into_values()
                .filter(|template| template.visibility == Visibility::Public && template.status == Status::Published)
                .collect();

            // Sort by most recently modified first (RFC 3339 strings are lexicographically comparable).
            public_templates.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

            public_templates
                .into_iter()
                .map(PublicTemplateDto::from)
                .collect::<Vec<PublicTemplateDto>>()
        })
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(public_templates)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_library::template::views::{all_templates::AllTemplatesView, TemplateView};
    use agent_store::{in_memory::InMemory, library_state};
    use axum::body::Body;
    use cqrs_es::persist::ViewContext;
    use http::Request;
    use tower::ServiceExt as _;

    fn template(template_id: &str, status: Status, visibility: Visibility, modified_at: &str) -> TemplateView {
        TemplateView {
            template_id: template_id.to_string(),
            title: format!("Title of {template_id}"),
            status,
            visibility,
            modified_at: Some(modified_at.to_string()),
            ..Default::default()
        }
    }

    /// Seeds the `all_templates` view directly so that each template's status, visibility and
    /// `modified_at` are fully controlled by the test.
    async fn app(templates: Vec<TemplateView>) -> axum::Router {
        let state = Arc::new(library_state(&InMemory, Default::default(), vec![]).await);

        let view = AllTemplatesView {
            templates: templates
                .into_iter()
                .map(|template| (template.template_id.clone(), template))
                .collect(),
        };
        state
            .query
            .all_templates
            .update_view(view, ViewContext::new("all_templates".to_string(), 0))
            .await
            .unwrap();

        crate::public::router(Some(state))
    }

    /// Templates have no public identifier, so tests identify them by their (unique) title.
    fn titles(body: &serde_json::Value) -> Vec<&str> {
        body.as_array()
            .unwrap()
            .iter()
            .map(|template| template["title"].as_str().unwrap())
            .collect()
    }

    async fn get_public_templates(app: axum::Router) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(Request::builder().uri("/public/templates").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();

        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn returns_only_public_and_published_templates() {
        let app = app(vec![
            template(
                "public-published",
                Status::Published,
                Visibility::Public,
                "2026-01-01T00:00:00Z",
            ),
            template(
                "public-draft",
                Status::Draft,
                Visibility::Public,
                "2026-01-01T00:00:00Z",
            ),
            template(
                "public-archived",
                Status::Archived,
                Visibility::Public,
                "2026-01-01T00:00:00Z",
            ),
            template(
                "public-deleted",
                Status::Deleted,
                Visibility::Public,
                "2026-01-01T00:00:00Z",
            ),
            template(
                "private-published",
                Status::Published,
                Visibility::Private,
                "2026-01-01T00:00:00Z",
            ),
        ])
        .await;

        let (status, body) = get_public_templates(app).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(titles(&body), vec!["Title of public-published"]);
    }

    #[tokio::test]
    async fn returns_an_empty_list_when_no_templates_are_public() {
        let app = app(vec![template(
            "private-published",
            Status::Published,
            Visibility::Private,
            "2026-01-01T00:00:00Z",
        )])
        .await;

        let (status, body) = get_public_templates(app).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!([]));
    }

    #[tokio::test]
    async fn returns_an_empty_list_when_the_view_does_not_exist_yet() {
        let state = Arc::new(library_state(&InMemory, Default::default(), vec![]).await);

        let (status, body) = get_public_templates(crate::public::router(Some(state))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!([]));
    }

    #[tokio::test]
    async fn sorts_by_most_recently_modified_first() {
        let app = app(vec![
            template("oldest", Status::Published, Visibility::Public, "2026-01-01T00:00:00Z"),
            template("newest", Status::Published, Visibility::Public, "2026-03-01T00:00:00Z"),
            template("middle", Status::Published, Visibility::Public, "2026-02-01T00:00:00Z"),
        ])
        .await;

        let (_, body) = get_public_templates(app).await;

        assert_eq!(
            titles(&body),
            vec!["Title of newest", "Title of middle", "Title of oldest"]
        );
    }

    #[tokio::test]
    async fn does_not_expose_internal_fields() {
        let app = app(vec![TemplateView {
            source_template_id: Some("the-source-template".to_string()),
            holder_authorization: agent_shared::config::Authorization {
                pre_authorized: false,
                tx_code_constraints: None,
            },
            description: Some("A description".to_string()),
            r#type: vec!["VerifiableCredential".to_string()],
            schema: Box::new(Some(serde_json::json!({ "type": "object" }))),
            ..template(
                "public-published",
                Status::Published,
                Visibility::Public,
                "2026-01-01T00:00:00Z",
            )
        }])
        .await;

        let (_, body) = get_public_templates(app).await;

        let template = &body.as_array().unwrap()[0];
        let keys: Vec<&String> = template.as_object().unwrap().keys().collect();

        for internal_field in ["id", "sourceTemplateId", "status", "visibility", "holderAuthorization"] {
            assert!(
                !keys.contains(&&internal_field.to_string()),
                "`{internal_field}` must not be exposed publicly, got: {keys:?}"
            );
        }

        // `modifiedAt` is deliberately public: it is the only freshness signal a reader has.
        assert_eq!(template["modifiedAt"], "2026-01-01T00:00:00Z");
        assert_eq!(template["title"], "Title of public-published");
        assert_eq!(template["description"], "A description");
        assert_eq!(template["type"], serde_json::json!(["VerifiableCredential"]));
        assert_eq!(template["schema"], serde_json::json!({ "type": "object" }));
    }
}
