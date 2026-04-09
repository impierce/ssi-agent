use crate::v0::holder::holder::credentials::{__path_credential, __path_credentials};
use crate::v0::holder::holder::offers::{__path_offer, __path_offers, accept::__path_accept, reject::__path_reject};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(credential, credentials, offer, offers, accept, reject),
    tags(
        (name = "Identity", description = "Manage all aspects of your organisational identity."),
        (name = "Holder", description = "Manage the credentials your organisation holds itself."),
    )
)]
pub struct HolderApi;
