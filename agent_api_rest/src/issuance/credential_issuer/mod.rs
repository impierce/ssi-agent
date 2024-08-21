pub mod credential;
pub mod token;
pub mod well_known;

#[derive(utoipa::OpenApi)]
#[openapi(
        paths(crate::issuance::credential_issuer::credential::credential),
        // components(schemas(Todo, TodoError))
)]
pub(crate) struct CredentialApi;
