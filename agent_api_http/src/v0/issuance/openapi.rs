use crate::v0::issuance::credential_configurations::__path_credential_configurations;
use crate::v0::issuance::credentials::{__path_all_credentials, __path_credential, __path_credentials};
use crate::v0::issuance::offers::{
    __path_all_offers, __path_offer,
    send::{__path_individual_offer, __path_organization_offer},
};
use crate::v0::issuance::public_offers::{
    __path_all_public_offers, __path_create_public_offer, __path_delete_public_offer,
    __path_take_public_offer_offline, __path_take_public_offer_online,
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
        all_public_offers,
        create_public_offer,
        take_public_offer_offline,
        take_public_offer_online,
        delete_public_offer
    ),
    tags(
        (name = "Credentials", description = "Create and revoke verifiable credentials."),
        (name = "Issuance", description = "Issue credentials to individuals and organizations, manage credential offers and track their status."),
    )
)]
pub struct IssuanceApi;
