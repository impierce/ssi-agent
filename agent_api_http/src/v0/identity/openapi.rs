use crate::v0::identity::documents::{__path_get_document, __path_get_documents};
use crate::v0::identity::profiles::{__path_get_profile, __path_patch_profile};
use crate::v0::identity::services::domain_linkage::{
    __path_create_domain_linkage, __path_remove_domain_linkage, __path_verify_domain_linkage,
};
use crate::v0::identity::services::linked_vp::{
    __path_create_linked_verifiable_presentation, __path_remove_linked_verifiable_presentation,
};
use crate::v0::identity::services::{__path_service, __path_services};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(get_document, get_documents, get_profile, patch_profile,
        services, service,
        create_domain_linkage, remove_domain_linkage, verify_domain_linkage,
        create_linked_verifiable_presentation, remove_linked_verifiable_presentation),
    tags(
        (name = "Identity", description = "Manage all aspects of your organisational identity."),
        (name = "Profile", description = "Manage your organisational profile."),
    )
)]
pub struct IdentityApi;
