use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(title = "UniCore HTTP API", license(name = "Apache 2.0"),),
    external_docs(
        description = "Official UniCore documentation",
        url = "https://docs.impierce.com/unicore"
    ),
    servers(
        (url = "http://localhost:3033", description = "Local development")
    ),
    nest(
        (path = "/v0", api = crate::v0::holder::openapi::HolderApi),
        (path = "/v0", api = crate::v0::identity::connections::openapi::ConnectionsApi),
        (path = "/v0", api = crate::v0::identity::openapi::IdentityApi),
        (path = "/v0", api = crate::v0::issuance::openapi::IssuanceApi),
        (path = "/v0", api = crate::v0::templates::openapi::TemplatesApi),
        (path = "/v0", api = crate::v0::library::catalog::openapi::CatalogsApi),
        (path = "/public", api = crate::public::openapi::PublicApi),
    )
)]
pub struct ApiDoc;

/// Applies manual adjustments to the generated OpenAPI specification.
#[allow(dead_code)]
fn patch_generated_openapi(mut spec: utoipa::openapi::OpenApi) -> utoipa::openapi::OpenApi {
    spec.info.version = std::env::var("APP_VERSION").unwrap_or_else(|_| "0.0.0-semantically-released".to_string());
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates an openapi.yaml file from the annotations in the code.
    #[test]
    fn generate_openapi_spec() {
        let openapi = patch_generated_openapi(ApiDoc::openapi());
        let yaml = openapi.to_yaml().unwrap();
        std::fs::write("openapi-generated.yaml", yaml).unwrap();
    }

    #[test]
    fn openapi_spec_is_up_to_date() {
        let current = std::fs::read_to_string("openapi-generated.yaml").unwrap();
        let latest = patch_generated_openapi(ApiDoc::openapi()).to_yaml().unwrap();
        assert_eq!(current, latest, "The OpenAPI specification is out of date. Please run the `generate_openapi_spec` test and commit the results.");
    }
}
