use crate::error::{type_url, IntoApiErrorExt};
use agent_issuance::{
    credential::error::CredentialError, offer::error::OfferError, server_config::error::ServerConfigError,
};
use axum::{response::IntoResponse, response::Response, Json};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::errors::{
    AuthorizationErrorResponse, BatchCredentialErrorResponse, CredentialErrorResponse, DeferredCredentialErrorResponse,
    ErrorStatusCode, NotificationErrorResponse, OID4VCError, TokenErrorResponse,
};
