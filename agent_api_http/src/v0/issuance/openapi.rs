use crate::v0::issuance::credentials::{
    __path_all_credentials, __path_credential, __path_credentials, __path_patch_credential,
};
use crate::v0::issuance::offers::{
    __path_all_offers, __path_offer, __path_offers,
    send::{__path_individual_offer, __path_organization_offer},
};
use crate::v0::issuance::{
    public_offers::{
        __path_all_public_offers, __path_create_public_offer, __path_delete_public_offer,
        __path_take_public_offer_offline, __path_take_public_offer_online,
    },
    reissuance::{__path_all_credential_reissuances, __path_credential_reissuance, __path_credential_reissuances},
};
use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        all_credentials,
        credential,
        credentials,
        patch_credential,
        all_offers,
        offer,
        offers,
        individual_offer,
        organization_offer,
        credential_reissuances,
        all_credential_reissuances,
        credential_reissuance,
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

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::v0::issuance::credential_issuer::well_known::oauth_authorization_server::oauth_authorization_server,
        crate::v0::issuance::credential_issuer::well_known::openid_credential_issuer::openid_credential_issuer,
        crate::v0::issuance::credential_issuer::credential::credential,
        crate::v0::issuance::nonce::nonce,
        crate::v0::issuance::credential_issuer::notification::notification,
        crate::v0::issuance::credential_issuer::credential_offer::credential_offer_uri,
        crate::v0::issuance::credential_issuer::token_status_list::token_status_list,
        crate::v0::issuance::ietf_oauth_sd_jwt_vc::type_metadata,
    ),
    modifiers(&AccessTokenSecurity)
)]
pub struct IssuanceProtocolApi;

/// Registers the OAuth 2.0 access token issued by `/auth/token` as a bearer security scheme.
struct AccessTokenSecurity;

impl Modify for AccessTokenSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi
            .components
            .get_or_insert_with(Default::default)
            .add_security_scheme(
                "access_token",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some("Access token issued by the `/auth/token` endpoint."))
                        .build(),
                ),
            );
    }
}
