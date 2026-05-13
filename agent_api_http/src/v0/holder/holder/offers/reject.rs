use crate::handlers::{command_handler, request_actor};
use agent_holder::{offer::command::OfferCommand, state::HolderState};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Extension,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use shared_kernel::authorization::Actor;
use std::sync::Arc;

/// Rejects a credential offer
///
/// Rejects a credential offered to your organization by its ID.
#[utoipa::path(
    post,
    path = "/holder/offers/{offer_id}/reject",
    operation_id = "reject_credential_offer",
    tags = ["Identity", "Holder"],
    responses(
        (status = 204, description = "Credential offer rejected successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn reject(
    State(state): State<Arc<HolderState>>,
    actor: Option<Extension<Option<Actor>>>,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::RejectCredentialOffer {
        received_offer_id: received_offer_id.clone(),
    };

    // Remove the Credential Offer from the state.
    command_handler(
        state.authorization_checker.clone(),
        request_actor(&actor),
        &received_offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_holder::services::HolderServices;
    use agent_secret_manager::service::Service;
    use agent_store::{holder_state, in_memory::InMemory};

    #[tokio::test]
    async fn reject_dispatches_reject_command() {
        let state = Arc::new(holder_state(&InMemory, HolderServices::default().await, Default::default()).await);

        let response = reject(State(state), None, Path("missing-offer".to_string()))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
