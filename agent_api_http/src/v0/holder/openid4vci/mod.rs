use crate::handlers::command_handler;
use agent_holder::{offer::command::OfferCommand, state::HolderState};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Form,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

#[axum_macros::debug_handler]
pub(crate) async fn offers_params(
    State(state): State<Arc<HolderState>>,
    // TODO: Can this be changed to `StringifiedForm`?
    Form(payload): Form<serde_json::Value>,
) -> Result<Response, ApiError> {
    let credential_offer_result: Result<CredentialOffer, _> =
        if let Some(credential_offer) = payload.get("credential_offer").and_then(Value::as_str) {
            format!("openid-credential-offer://?credential_offer={credential_offer}")
        } else if let Some(credential_offer_uri) = payload.get("credential_offer_uri").and_then(Value::as_str) {
            format!("openid-credential-offer://?credential_offer_uri={credential_offer_uri}")
        } else {
            return Err(ApiError::new(StatusCode::BAD_REQUEST));
        }
        .parse();

    let credential_offer = match credential_offer_result {
        Ok(credential_offer) => credential_offer,
        Err(_) => return Err(ApiError::new(StatusCode::BAD_REQUEST)),
    };

    let received_offer_id = uuid::Uuid::new_v4().to_string();

    info!("Credential Offer: {:#?}", credential_offer);

    let command = OfferCommand::ReceiveCredentialOffer {
        received_offer_id: received_offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    command_handler(&state, &received_offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_holder::services::HolderServices;
    use agent_secret_manager::service::Service;
    use agent_store::{holder_state, in_memory::InMemory};
    use oid4vci::credential_offer::{CredentialConfigurationIds, CredentialOfferParameters};
    use serde_json::json;

    #[tokio::test]
    async fn offers_params_dispatches_receive_offer_command() {
        let state = Arc::new(holder_state(&InMemory, HolderServices::default().await, Default::default()).await);
        let credential_offer = serde_json::to_string(&CredentialOfferParameters {
            credential_issuer: "http://127.0.0.1:9".parse().unwrap(),
            credential_configuration_ids: CredentialConfigurationIds::try_new(vec!["001".to_string()]).unwrap(),
            grants: None,
        })
        .unwrap();

        let error = offers_params(
            State(state),
            Form(json!({
                "credential_offer": urlencoding::encode(&credential_offer).to_string(),
            })),
        )
        .await
        .unwrap_err();

        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
