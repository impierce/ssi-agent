use crate::v0::verification::authorization_requests::{
    __path_all_authorization_requests, __path_authorization_request, __path_authorization_requests,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(all_authorization_requests, authorization_request, authorization_requests),
    tags(
        (name = "Authorization Requests", description = "Manage requests for verifiable presentations and their responses."),
    )
)]
pub struct VerificationApi;

#[derive(OpenApi)]
#[openapi(paths(
    crate::v0::verification::relying_party::request::request,
    crate::v0::verification::relying_party::redirect::redirect,
))]
pub struct VerificationProtocolApi;
