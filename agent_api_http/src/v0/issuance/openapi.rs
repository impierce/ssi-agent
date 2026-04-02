use crate::v0::issuance::credentials::{__path_all_credentials, __path_credential};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(all_credentials, credential),
    tags(
        (name = "Issuance", description = "Issue credentials to individuals and organizations, manage credential offers and track their status."),
    )
)]
pub struct IssuanceApi;
