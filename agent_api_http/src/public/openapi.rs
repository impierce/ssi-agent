use crate::public::{
    sponsoring_configuration::__path_sponsoring_configuration, templates::__path_get_public_templates,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_public_templates, sponsoring_configuration),
    tags(
        (name = "Public", description = "Endpoints that are publicly reachable without authentication, used to expose publicly available information or communicate with external systems.")
    )
)]
pub struct PublicApi;
