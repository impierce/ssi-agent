pub mod oauth_authorization_server;
pub mod openid_credential_issuer;

#[derive(utoipa::OpenApi)]
#[openapi(
        paths(crate::issuance::credential_issuer::well_known::oauth_authorization_server::oauth_authorization_server),
        // components(schemas(Todo, TodoError))
)]
pub(crate) struct WellKnownApi;
