use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(
    crate::v0::authorization::authorization_server::consent::get_consent,
    crate::v0::authorization::authorization_server::consent::post_consent,
    crate::v0::authorization::authorization_server::par::par,
    crate::v0::authorization::authorization_server::authorize::authorize,
    crate::v0::authorization::authorization_server::token::token,
))]
pub struct AuthorizationProtocolApi;
