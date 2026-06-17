use crate::v0::issuance::credential_configurations::__path_credential_configurations;
use crate::v0::issuance::credentials::{__path_all_credentials, __path_credential, __path_credentials};
use crate::v0::issuance::offers::{
    __path_all_offers, __path_offer,
    send::{__path_individual_offer, __path_organization_offer},
};
use crate::v0::issuance::reissuance::{
    __path_all_credential_reissuances, __path_credential_reissuance, __path_credential_reissuances,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        credential_configurations,
        all_credentials,
        credential,
        credentials,
        all_offers,
        offer,
        individual_offer,
        organization_offer,
        credential_reissuances,
        all_credential_reissuances,
        credential_reissuance
    ),
    tags(
        (name = "Credentials", description = "Create and revoke verifiable credentials."),
        (name = "Issuance", description = "Issue credentials to individuals and organizations, manage credential offers and track their status."),
    )
)]
pub struct IssuanceApi;
