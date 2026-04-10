use crate::v0::issuance::credentials::{__path_all_credentials, __path_credential, __path_credentials};
use crate::v0::issuance::offers::{
    __path_all_offers, __path_offer,
    send::{__path_individual_offer, __path_organization_offer},
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(all_credentials, credential, credentials, all_offers, offer, individual_offer, organization_offer),
    tags(
        (name = "Credentials", description = "Create and revoke verifiable credentials."),
        (name = "Issuance", description = "Issue credentials to individuals and organizations, manage credential offers and track their status."),
    )
)]
pub struct IssuanceApi;
