use crate::handlers::command_handler;
use agent_issuance::{offer::aggregate::DeliveryMethod, offer::command::OfferCommand, state::IssuanceState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension, Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use shared_kernel::authorization::Actor;
use std::sync::Arc;
use url::Url;

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailOfferEndpointRequest {
    pub offer_id: String,
    pub recipient_email: String,
}

/// Send offer to individual
///
/// Sends a credential offer to an individual's email.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-individual",
    operation_id = "send_offer_to_individual",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn individual_offer(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(EmailOfferEndpointRequest {
        offer_id,
        recipient_email,
    }): Json<EmailOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::Email { recipient_email },
    };

    // Send the Credential Offer to the recipient's email.
    command_handler(
        state.authorization_checker.clone(),
        actor.clone().and_then(|Extension(actor)| actor),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetUrlOfferEndpointRequest {
    pub offer_id: String,
    pub target_url: Url,
}

/// Send offer to organization
///
/// Sends a credential offer to an organization's URL.
#[utoipa::path(
    post,
    path = "/offers/send-offer-to-organization",
    operation_id = "send_offer_to_organization",
    tags = ["Issuance"],
    responses(
        (status = 200, description = "Offer sent successfully")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn organization_offer(
    State(state): State<Arc<IssuanceState>>,
    actor: Option<Extension<Option<Actor>>>,
    Json(TargetUrlOfferEndpointRequest { offer_id, target_url }): Json<TargetUrlOfferEndpointRequest>,
) -> Result<Response, ApiError> {
    let command = OfferCommand::SendCredentialOffer {
        offer_id: offer_id.clone(),
        delivery_method: DeliveryMethod::TargetUrl { target_url },
    };

    // Send the offer to the organizational url.
    command_handler(
        state.authorization_checker.clone(),
        actor.clone().and_then(|Extension(actor)| actor),
        &offer_id,
        &state.command.offer,
        command,
    )
    .await?;

    Ok(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_issuance::{services::IssuanceServices, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::{in_memory::InMemory, issuance_state};

    async fn test_state() -> Arc<IssuanceState> {
        let state = Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        initialize(&state).await.unwrap();
        state
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn individual_offer_dispatches_send_command() {
        let error = individual_offer(
            State(test_state().await),
            None,
            Json(EmailOfferEndpointRequest {
                offer_id: crate::tests::OFFER_ID.to_string(),
                recipient_email: "receiver@example.com".to_string(),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn organization_offer_dispatches_send_command() {
        let error = organization_offer(
            State(test_state().await),
            None,
            Json(TargetUrlOfferEndpointRequest {
                offer_id: crate::tests::OFFER_ID.to_string(),
                target_url: "https://receiver.example.com/offers".parse().unwrap(),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }
}
