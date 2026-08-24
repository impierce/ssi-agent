use crate::public::templates::__path_get_public_templates;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_public_templates),
    tags(
        (name = "Public", description = "Endpoints that are publicly reachable without authentication, used to expose publicly available information or communicate with external systems.")
    )
)]
pub struct PublicApi;
