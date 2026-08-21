use crate::public::templates::__path_get_public_templates;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_public_templates),
    tags(
        (name = "Public", description = "Unauthenticated endpoints exposing publicly available information.")
    )
)]
pub struct PublicApi;
