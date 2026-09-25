use crate::{
    metadata::{info::__path_info, version::__path_version},
    probes::{
        liveness::{__path_healthz, __path_livez},
        readiness::__path_readyz,
    },
};
use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(version, info, healthz, livez, readyz, openapi_yaml),
    tags(
        (name = "Metadata", description = "Inspect application build and runtime metadata."),
        (name = "Probes", description = "Inspect application liveness and readiness."),
        (name = "OpenAPI", description = "Inspect the OpenAPI description of this application."),
    )
)]
pub struct OperationalApi;

pub struct PublishedApiDoc;

impl OpenApi for PublishedApiDoc {
    fn openapi() -> utoipa::openapi::OpenApi {
        agent_api_http::v0::openapi::ApiDoc::openapi().merge_from(OperationalApi::openapi())
    }
}

pub fn published_openapi() -> utoipa::openapi::OpenApi {
    patch_generated_openapi(PublishedApiDoc::openapi())
}

fn patch_generated_openapi(mut spec: utoipa::openapi::OpenApi) -> utoipa::openapi::OpenApi {
    spec.info.version = std::env::var("APP_VERSION").unwrap_or_else(|_| "0.0.0-semantically-released".to_string());
    spec
}

pub fn router(enabled: bool) -> Router {
    if enabled {
        Router::new().route("/openapi.yaml", get(openapi_yaml))
    } else {
        Router::new()
    }
}

/// Get the OpenAPI document
///
/// Returns the OpenAPI description of the endpoints published by this application.
#[utoipa::path(
    get,
    path = "/openapi.yaml",
    operation_id = "openapi_yaml",
    tags = ["OpenAPI"],
    responses(
        (status = 200, description = "OpenAPI document", body = String, content_type = "application/yaml"),
        (status = 500, description = "OpenAPI document serialization failed"),
    )
)]
async fn openapi_yaml() -> Result<impl IntoResponse, StatusCode> {
    let yaml = published_openapi()
        .to_yaml()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(([(header::CONTENT_TYPE, "application/yaml")], yaml))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::collections::BTreeSet;
    use tower::ServiceExt as _;
    use utoipa::openapi::{path::HttpMethod, OpenApi as OpenApiDocument};

    type Operation = (String, String, String);

    fn operations(document: &OpenApiDocument) -> BTreeSet<Operation> {
        document
            .paths
            .paths
            .iter()
            .flat_map(|(path, item)| {
                [
                    ("get", item.get.as_ref()),
                    ("put", item.put.as_ref()),
                    ("post", item.post.as_ref()),
                    ("delete", item.delete.as_ref()),
                    ("options", item.options.as_ref()),
                    ("head", item.head.as_ref()),
                    ("patch", item.patch.as_ref()),
                    ("trace", item.trace.as_ref()),
                ]
                .into_iter()
                .filter_map(|(method, operation)| {
                    operation.map(|operation| {
                        (
                            method.to_string(),
                            path.clone(),
                            operation.operation_id.clone().unwrap_or_default(),
                        )
                    })
                })
            })
            .collect()
    }

    fn schema_names(document: &OpenApiDocument) -> BTreeSet<String> {
        document
            .components
            .as_ref()
            .map(|components| components.schemas.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn published_api_adds_only_the_shared_operations_and_schemas() {
        let api_http = agent_api_http::v0::openapi::ApiDoc::openapi();
        let published = PublishedApiDoc::openapi();

        let api_http_operations = operations(&api_http);
        let published_operations = operations(&published);
        let operational_additions: BTreeSet<_> =
            published_operations.difference(&api_http_operations).cloned().collect();

        assert_eq!(
            operational_additions,
            BTreeSet::from([
                ("get".to_string(), "/healthz".to_string(), "healthz".to_string()),
                ("get".to_string(), "/info".to_string(), "info".to_string()),
                ("get".to_string(), "/livez".to_string(), "livez".to_string()),
                (
                    "get".to_string(),
                    "/openapi.yaml".to_string(),
                    "openapi_yaml".to_string(),
                ),
                ("get".to_string(), "/readyz".to_string(), "readyz".to_string()),
                ("get".to_string(), "/version".to_string(), "version".to_string()),
            ])
        );
        assert!(api_http_operations.is_subset(&published_operations));
        assert_eq!(published_operations.len(), api_http_operations.len() + 6);

        let sponsoring_configuration = api_http
            .paths
            .get_path_operation("/public/sponsoring-configuration", HttpMethod::Get)
            .expect("sponsoring configuration operation should be registered");
        assert_eq!(
            sponsoring_configuration.operation_id.as_deref(),
            Some("sponsoring_configuration")
        );

        let api_http_schemas = schema_names(&api_http);
        assert!(api_http_schemas.contains("SponsoringConfiguration"));

        let published_schemas = schema_names(&published);
        let operational_schemas: BTreeSet<_> = published_schemas.difference(&api_http_schemas).cloned().collect();
        assert_eq!(
            operational_schemas,
            BTreeSet::from([
                "ApplicationProfile".to_string(),
                "Info".to_string(),
                "Version".to_string(),
            ])
        );
        assert!(api_http_schemas.is_subset(&published_schemas));
        assert_eq!(published_schemas.len(), api_http_schemas.len() + 3);

        assert!(published.info == api_http.info);
        assert!(published.servers == api_http.servers);
        assert!(published.external_docs == api_http.external_docs);
    }

    #[tokio::test]
    async fn openapi_yaml_route_is_disabled_by_default() {
        let response = router(false)
            .oneshot(Request::builder().uri("/openapi.yaml").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn openapi_yaml_route_serves_the_published_document_when_enabled() {
        let response = router(true)
            .oneshot(Request::builder().uri("/openapi.yaml").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/yaml"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("  /openapi.yaml:"));
        assert!(body.contains("operationId: openapi_yaml"));
    }

    #[test]
    fn generate_openapi_spec() {
        let openapi = published_openapi();
        let yaml = openapi.to_yaml().unwrap();
        let output_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent_api_http/openapi.yaml");
        std::fs::write(output_path, yaml).unwrap();
    }
}
