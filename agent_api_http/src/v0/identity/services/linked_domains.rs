use crate::{error::IntoApiErrorExt, extractors::RequestActor};
use agent_identity::{
    document::web::normalize_origin,
    service::{command::ServiceCommand, lifecycle},
    state::{IdentityState, LINKED_DOMAINS_SERVICE_ID},
};
use axum::{extract::State, Json};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::Deserialize;
use std::sync::Arc;
use url::Url;

/// The origins to link or unlink, each either a bare host (`example.org`) or a full origin
/// (`https://example.org`). Independent of the deployment's own DID.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LinkedDomainsRequest {
    #[schema(example = json!(["example.org", "https://foo.example.org"]))]
    pub origins: Vec<String>,
}

impl LinkedDomainsRequest {
    /// Normalizes and validates every origin, rejecting the whole request if any one of them is not
    /// a usable origin. An empty list is left to the aggregate, which rejects it for both endpoints.
    #[allow(clippy::result_large_err)]
    fn origins(&self) -> Result<Vec<Url>, ApiError> {
        self.origins
            .iter()
            .map(|origin| normalize_origin(origin).map_err(IntoApiErrorExt::into_api_error))
            .collect()
    }
}

/// Link domains
///
/// Publishes a Domain Linkage Credential for each given origin, in addition to any already linked.
/// Linking an origin that is already linked is a no-op.
#[utoipa::path(
    post,
    path = "/add-linked-domains",
    operation_id = "add_linked_domains",
    tags = ["Identity"],
    request_body(content = inline(LinkedDomainsRequest), content_type = "application/json"),
    responses(
        (status = 204, description = "Domains linked"),
        (status = 400, description = "No eligible signing DID, or an invalid origin"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn add_linked_domains(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(request): Json<LinkedDomainsRequest>,
) -> Result<StatusCode, ApiError> {
    let origins = request.origins()?;

    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::AddLinkedDomains {
            service_id: LINKED_DOMAINS_SERVICE_ID.into(),
            verification_methods: vec![],
            origins,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Unlink domains
///
/// Withdraws the Domain Linkage Credentials for each given origin. Unlinking an origin that is not
/// linked is a no-op, and unlinking the last remaining origin removes the service entirely.
#[utoipa::path(
    post,
    path = "/remove-linked-domains",
    operation_id = "remove_linked_domains",
    tags = ["Identity"],
    request_body(content = inline(LinkedDomainsRequest), content_type = "application/json"),
    responses(
        (status = 204, description = "Domains unlinked"),
        (status = 400, description = "An invalid origin"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn remove_linked_domains(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
    Json(request): Json<LinkedDomainsRequest>,
) -> Result<StatusCode, ApiError> {
    let origins = request.origins()?;

    lifecycle::execute(
        &state,
        actor,
        ServiceCommand::RemoveLinkedDomains {
            service_id: LINKED_DOMAINS_SERVICE_ID.into(),
            origins,
        },
    )
    .await
    .map_err(|error| error.into_api_error())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Verify linked domains
///
/// Checks every linked domain the way an external verifier would: resolves each origin's published
/// DID configuration over the network and validates it, alongside a freshly resolved `CNAME` lookup
/// showing whether the domain points at this deployment.
#[utoipa::path(
    get,
    path = "/verify-linked-domains",
    operation_id = "verify_linked_domains",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Verification result per linked domain", body = lifecycle::LinkedDomainsVerification),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn verify_linked_domains(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Json<lifecycle::LinkedDomainsVerification>, ApiError> {
    lifecycle::verify(&state, actor)
        .await
        .map(Json)
        .map_err(|error| error.into_api_error())
}
