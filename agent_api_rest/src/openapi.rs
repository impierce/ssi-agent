use utoipa::openapi::path::OperationBuilder;
use utoipa::openapi::{Content, HttpMethod, PathItem, Ref, Response, ResponseBuilder, ResponsesBuilder};
use utoipa::OpenApi;

use crate::holder::holder;
use crate::issuance::credentials::{self, CredentialsEndpointRequest};
use crate::issuance::offers;
use crate::verification::authorization_requests;

#[derive(OpenApi)]
#[openapi(
    paths(
        credentials::credentials,
        credentials::get_credentials,
        offers::offers,
        offers::send::send
    ),
    components(schemas(CredentialsEndpointRequest))
)]
pub(crate) struct IssuanceApi;

#[derive(OpenApi)]
#[openapi(paths(
    authorization_requests::authorization_requests,
    authorization_requests::get_authorization_requests
))]
pub(crate) struct VerificationApi;

#[derive(OpenApi)]
#[openapi(paths(
    holder::credentials::credentials,
    holder::offers::offers,
    holder::offers::accept::accept,
    holder::offers::reject::reject
))]
pub(crate) struct HolderApi;

pub(crate) fn did_web() -> PathItem {
    PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .responses(
                ResponsesBuilder::new()
                    .response(
                        "200",
                        ResponseBuilder::new()
                            .description("DID Document for `did:web` method")
                            .content("application/json", Content::new(Ref::from_schema_name("CoreDocument"))),
                    )
                    .response("404", Response::new("DID method `did:web` inactive.")),
            )
            .operation_id(Some("did_json"))
            .summary(Some("DID Document for `did:web` method"))
            .description(Some("Standard .well-known endpoint for self-hosted DID Document."))
            .tags(Some(vec!["(.well-known)", "(public)"])),
    )
}

pub(crate) fn did_configuration() -> PathItem {
    PathItem::new(
        HttpMethod::Get,
        OperationBuilder::new()
            .responses(
                ResponsesBuilder::new()
                    .response(
                        "200",
                        ResponseBuilder::new()
                            .description("DID Configuration Resource")
                            .content(
                                "application/json",
                                Content::new(Ref::from_schema_name("DomainLinkageConfiguration")),
                            ),
                    )
                    .response("404", Response::new("Domain Linkage inactive.")),
            )
            .operation_id(Some("did_configuration_json"))
            .summary(Some("DID Configuration Resource for Domain Linkage"))
            .description(Some("Standard .well-known endpoint for DID Configuration Resources."))
            .tags(Some(vec!["(.well-known)", "(public)"])),
    )
}
