use utoipa::OpenApi;

use crate::holder::holder;
use crate::issuance::credential_issuer::well_known::{oauth_authorization_server, openid_credential_issuer};
use crate::issuance::credentials::{self, CredentialsEndpointRequest};
use crate::issuance::offers;
use crate::verification::authorization_requests;

#[derive(OpenApi)]
#[openapi(
        // paths(credential::credential, credentials, get_credentials),
        paths(credentials::credentials, credentials::get_credentials, offers::offers),
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

#[derive(OpenApi)]
#[openapi(
        paths(oauth_authorization_server::oauth_authorization_server, openid_credential_issuer::openid_credential_issuer),
        // components(schemas(Todo, TodoError))
)]
pub(crate) struct WellKnownApi;