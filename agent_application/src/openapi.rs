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

/// The published document plus every standardized protocol endpoint.
pub struct FullApiDoc;

impl OpenApi for FullApiDoc {
    fn openapi() -> utoipa::openapi::OpenApi {
        PublishedApiDoc::openapi().merge_from(agent_api_http::v0::openapi::ProtocolApi::openapi())
    }
}

pub fn full_openapi() -> utoipa::openapi::OpenApi {
    patch_generated_openapi(FullApiDoc::openapi())
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

    /// Every operation of the document together with its tags.
    fn tagged_operations(document: &OpenApiDocument) -> Vec<(Operation, Vec<String>)> {
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
                            (
                                method.to_string(),
                                path.clone(),
                                operation.operation_id.clone().unwrap_or_default(),
                            ),
                            operation.tags.clone().unwrap_or_default(),
                        )
                    })
                })
            })
            .collect()
    }

    fn operations(document: &OpenApiDocument) -> BTreeSet<Operation> {
        tagged_operations(document)
            .into_iter()
            .map(|(operation, _)| operation)
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

    /// The audited `(method, path, operation_id)` manifest of the standardized protocol operations. Axum cannot
    /// enumerate its routes, so any change to the protocol routers must be reflected here.
    fn protocol_operations() -> BTreeSet<Operation> {
        [
            ("get", "/.well-known/did.json", "well_known_did_json"),
            (
                "get",
                "/.well-known/did-configuration.json",
                "well_known_did_configuration_json",
            ),
            (
                "get",
                "/.well-known/oauth-authorization-server",
                "well_known_oauth_authorization_server",
            ),
            (
                "get",
                "/.well-known/openid-credential-issuer",
                "well_known_openid_credential_issuer",
            ),
            ("post", "/openid4vci/credential", "openid4vci_credential"),
            ("post", "/openid4vci/nonce", "openid4vci_nonce"),
            ("post", "/openid4vci/notification", "openid4vci_notification"),
            (
                "get",
                "/openid4vci/credential-offer/{offer_id}",
                "openid4vci_credential_offer",
            ),
            ("get", "/ietf-oauth-token-status-list/{path}", "token_status_list"),
            (
                "get",
                "/vct/{credential_configuration_id}/{version}",
                "vct_type_metadata",
            ),
            ("get", "/auth/consent", "get_consent"),
            ("post", "/auth/consent", "post_consent"),
            ("post", "/auth/par", "auth_par"),
            ("get", "/auth/authorize", "auth_authorize"),
            ("post", "/auth/token", "auth_token"),
            ("get", "/credential_offer", "credential_offer"),
            (
                "get",
                "/linked-verifiable-presentations/{presentation_id}",
                "linked_verifiable_presentation",
            ),
            ("get", "/request/{request_id}", "request_object"),
            ("post", "/redirect", "redirect"),
        ]
        .into_iter()
        .map(|(method, path, operation_id)| (method.to_string(), path.to_string(), operation_id.to_string()))
        .collect()
    }

    #[test]
    fn full_api_adds_exactly_the_protocol_operations() {
        let published = PublishedApiDoc::openapi();
        let full = FullApiDoc::openapi();

        let published_operations = operations(&published);
        let full_operations = operations(&full);
        assert!(published_operations.is_subset(&full_operations));

        let protocol_additions: BTreeSet<_> = full_operations.difference(&published_operations).cloned().collect();
        assert_eq!(protocol_additions, protocol_operations());
        assert!(published_operations.is_disjoint(&protocol_operations()));

        let operation_ids: Vec<_> = full_operations
            .iter()
            .map(|(_, _, operation_id)| operation_id)
            .collect();
        let unique_operation_ids: BTreeSet<_> = operation_ids.iter().collect();
        assert_eq!(
            operation_ids.len(),
            unique_operation_ids.len(),
            "operation IDs must be unique"
        );

        assert!(full.info == published.info);
        assert!(full.servers == published.servers);
    }

    #[test]
    fn only_protocol_operations_carry_the_protocol_tag() {
        use agent_api_http::v0::openapi::PROTOCOL_TAG;

        let protocol_operations = protocol_operations();

        for (operation, tags) in tagged_operations(&FullApiDoc::openapi()) {
            if protocol_operations.contains(&operation) {
                // The standard's own tag comes first, because client generators group operations by their first tag.
                assert!(
                    tags.len() >= 2 && tags.last().map(String::as_str) == Some(PROTOCOL_TAG),
                    "{operation:?} should end with the `{PROTOCOL_TAG}` tag after its standard's tag, got {tags:?}"
                );
                assert_eq!(tags.iter().filter(|tag| *tag == PROTOCOL_TAG).count(), 1);
            } else {
                assert!(
                    !tags.iter().any(|tag| tag == PROTOCOL_TAG),
                    "{operation:?} is not a protocol operation"
                );
            }
        }

        let full = FullApiDoc::openapi();
        let tag_names: Vec<_> = full.tags.iter().flatten().map(|tag| tag.name.as_str()).collect();
        assert!(tag_names.contains(&PROTOCOL_TAG));
        assert!(!PublishedApiDoc::openapi()
            .tags
            .iter()
            .flatten()
            .any(|tag| tag.name == PROTOCOL_TAG));
    }

    #[test]
    fn protocol_schemas_do_not_shadow_published_schemas() {
        let published = PublishedApiDoc::openapi();
        let protocol = agent_api_http::v0::openapi::ProtocolApi::openapi();
        let full = FullApiDoc::openapi();

        let schemas = |document: &OpenApiDocument| {
            document
                .components
                .as_ref()
                .map(|components| components.schemas.clone())
                .unwrap_or_default()
        };
        let published_schemas = schemas(&published);
        let full_schemas = schemas(&full);

        // `merge_from` silently keeps the first schema of a given name, so a name collision must be an identical
        // schema.
        let shadowed: Vec<_> = schemas(&protocol)
            .into_iter()
            .filter(|(name, schema)| {
                published_schemas.get(name).is_some_and(|published_schema| {
                    serde_json::to_value(published_schema).unwrap() != serde_json::to_value(schema).unwrap()
                })
            })
            .map(|(name, _)| name)
            .collect();
        assert!(
            shadowed.is_empty(),
            "protocol schemas differ from published schemas of the same name: {shadowed:?}"
        );
        assert!(schemas(&protocol).keys().all(|name| full_schemas.contains_key(name)));
    }

    #[test]
    fn generate_openapi_spec() {
        let output_directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent_api_http");

        for (file_name, openapi) in [
            ("openapi.yaml", published_openapi()),
            ("openapi-full.yaml", full_openapi()),
        ] {
            let yaml = openapi.to_yaml().unwrap();
            assert_eq!(
                yaml,
                openapi.to_yaml().unwrap(),
                "{file_name} must serialize deterministically"
            );
            std::fs::write(output_directory.join(file_name), yaml).unwrap();
        }
    }
}
