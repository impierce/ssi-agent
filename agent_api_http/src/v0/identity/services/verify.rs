use crate::{error::IntoApiErrorExt, extractors::RequestActor};
use agent_identity::{service::lifecycle, state::IdentityState};
use axum::{extract::State, Json};
use http_api_problem::ApiError;
use std::sync::Arc;

#[utoipa::path(
    post,
    path = "/verify-domain-linkage",
    operation_id = "verify_domain_linkage",
    tags = ["Identity"],
    responses(
        (status = 200, description = "Domain linkage verification result", body = lifecycle::DomainLinkageVerification),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Operation forbidden"),
    )
)]
pub(crate) async fn verify_domain_linkage(
    State(state): State<Arc<IdentityState>>,
    RequestActor(actor): RequestActor,
) -> Result<Json<lifecycle::DomainLinkageVerification>, ApiError> {
    lifecycle::verify(&state, actor)
        .await
        .map(Json)
        .map_err(|error| error.into_api_error())
}
