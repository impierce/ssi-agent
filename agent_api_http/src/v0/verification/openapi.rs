use crate::v0::verification::authorization_requests::{
    __path_all_authorization_requests, __path_authorization_request, __path_authorization_requests,
};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(all_authorization_requests, authorization_request, authorization_requests),
    tags(
        (name = "Verification", description = "Manage requests for verifiable presentations and their responses."),
    )
)]
pub struct VerificationApi;
