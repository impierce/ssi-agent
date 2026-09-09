use crate::v0::holder::holder::credentials::{__path_credential, __path_credentials, __path_post_credentials};
use crate::v0::holder::holder::offers::{__path_offer, __path_offers, accept::__path_accept, reject::__path_reject};
use crate::v0::holder::holder::presentations::{
    __path_get_presentations, __path_post_presentations, __path_presentation,
    presentation_signed::__path_presentation_signed,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        credential,
        credentials,
        post_credentials,
        offer,
        offers,
        accept,
        reject,
        get_presentations,
        post_presentations,
        presentation,
        presentation_signed
    ),
    tags(
        (name = "Identity", description = "Manage all aspects of your organisational identity."),
        (name = "Holder", description = "Manage the credentials your organisation holds itself."),
    )
)]
pub struct HolderApi;
