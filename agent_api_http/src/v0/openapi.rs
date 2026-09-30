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
        (path = "/v0", api = crate::v0::configuration::ConfigurationApi),
        (path = "/v0", api = crate::v0::holder::openapi::HolderApi),
        (path = "/v0", api = crate::v0::events::openapi::EventsApi),
        (path = "/v0", api = crate::v0::identity::connections::openapi::ConnectionsApi),
        (path = "/v0", api = crate::v0::identity::openapi::IdentityApi),
        (path = "/v0", api = crate::v0::issuance::openapi::IssuanceApi),
        (path = "/v0", api = crate::v0::templates::openapi::TemplatesApi),
        (path = "/v0", api = crate::v0::library::catalog::openapi::CatalogsApi),
        (path = "/v0", api = crate::v0::verification::openapi::VerificationApi),
        (path = "/public", api = crate::public::openapi::PublicApi),
    )
)]
pub struct ApiDoc;

/// The tag that every operation of [`ProtocolApi`] carries after the tag of its standard, since client generators
/// group operations by their first tag.
pub const PROTOCOL_TAG: &str = "Protocol";

/// The standardized protocol endpoints, which are only part of the full OpenAPI document.
#[derive(OpenApi)]
#[openapi(tags(
    (name = PROTOCOL_TAG, description = "Endpoints defined by external standards, used by wallets, verifiers and resolvers."),
    (name = "DID", description = "Resolve the DID documents and domain linkage of this application."),
    (name = "OAuth 2.0", description = "Authorize wallets through the OAuth 2.0 authorization server."),
    (name = "OpenID4VCI", description = "Issue and receive credentials with OpenID for Verifiable Credential Issuance."),
    (name = "OID4VP / SIOPv2", description = "Request and verify presentations with OpenID for Verifiable Presentations and Self-Issued OpenID Provider v2."),
    (name = "Status List", description = "Retrieve credential status lists as defined by OAuth Token Status List."),
    (name = "SD-JWT VC", description = "Retrieve SD-JWT VC type metadata."),
))]
struct ProtocolTags;

pub struct ProtocolApi;

impl OpenApi for ProtocolApi {
    fn openapi() -> utoipa::openapi::OpenApi {
        [
            crate::v0::identity::openapi::IdentityProtocolApi::openapi(),
            crate::v0::issuance::openapi::IssuanceProtocolApi::openapi(),
            crate::v0::authorization::openapi::AuthorizationProtocolApi::openapi(),
            crate::v0::holder::openapi::HolderProtocolApi::openapi(),
            crate::v0::verification::openapi::VerificationProtocolApi::openapi(),
        ]
        .into_iter()
        .fold(ProtocolTags::openapi(), utoipa::openapi::OpenApi::merge_from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Components are merged by name, so two different schemas registered under one name silently replace each other.
    #[test]
    fn component_names_are_unambiguous() {
        let documents = [
            crate::v0::configuration::ConfigurationApi::openapi(),
            crate::v0::holder::openapi::HolderApi::openapi(),
            crate::v0::events::openapi::EventsApi::openapi(),
            crate::v0::identity::connections::openapi::ConnectionsApi::openapi(),
            crate::v0::identity::openapi::IdentityApi::openapi(),
            crate::v0::issuance::openapi::IssuanceApi::openapi(),
            crate::v0::templates::openapi::TemplatesApi::openapi(),
            crate::v0::library::catalog::openapi::CatalogsApi::openapi(),
            crate::v0::verification::openapi::VerificationApi::openapi(),
            crate::public::openapi::PublicApi::openapi(),
            ProtocolApi::openapi(),
        ];

        let mut schemas = BTreeMap::new();
        let mut ambiguous = Vec::new();
        for (name, schema) in documents.into_iter().flat_map(|document| {
            document
                .components
                .map(|components| components.schemas)
                .unwrap_or_default()
        }) {
            let schema = serde_json::to_value(schema).unwrap();
            match schemas.get(&name) {
                Some(existing) if *existing != schema => ambiguous.push(name),
                Some(_) => {}
                None => {
                    schemas.insert(name, schema);
                }
            }
        }

        assert!(ambiguous.is_empty(), "ambiguous component names: {ambiguous:?}");
    }
}
