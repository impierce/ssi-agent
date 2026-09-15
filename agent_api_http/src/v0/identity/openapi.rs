use crate::v0::identity::documents::{__path_get_document, __path_get_documents};
use crate::v0::identity::profiles::{__path_get_profile, __path_patch_profile};
use crate::v0::identity::services::{__path_service, __path_services, linked_vp::__path_linked_vp};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        get_document,
        get_documents,
        get_profile,
        patch_profile,
        services,
        service,
        linked_vp
    ),
    tags(
        (name = "Identity", description = "Manage all aspects of your organisational identity."),
        (name = "Profile", description = "Manage your organisational profile."),
    )
)]
pub struct IdentityApi;
