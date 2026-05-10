use crate::handlers::{command_handler, query_handler};
use agent_holder::{
    credential::command::CredentialCommand,
    offer::{
        aggregate::{Offer, OfferCredential},
        command::OfferCommand,
        queries::ReceivedOfferView,
    },
    state::HolderState,
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use std::sync::Arc;

/// Accept a credential offer
///
/// Accepts a credential offered to your organization by its ID.
#[utoipa::path(
    post,
    path = "/holder/offers/{offer_id}/accept",
    operation_id = "accept_credential_offer",
    tags = ["Identity", "Holder"],
    responses(
        (status = 201, description = "Credential offer accepted successfully", body = Offer),
        (status = 404, description = "Credential offer not found"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn accept(
    State(state): State<Arc<HolderState>>,
    Path(received_offer_id): Path<String>,
) -> Result<Response, ApiError> {
    // TODO: General note that also applies to other endpoints: currently we are using Application Layer logic in the
    // REST API. This is not ideal and should be changed. The REST API should only be responsible for handling HTTP
    // Requests and Responses.
    // Furthermore, the Application Layer (not implemented yet) should be kept very thin as well. See: https://github.com/impierce/ssi-agent/issues/114

    // Check if the Credential Offer exists.
    query_handler(&received_offer_id, &state.query.received_offer)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))?;

    let command = OfferCommand::AcceptCredentialOffer {
        received_offer_id: received_offer_id.clone(),
    };

    // Accept the Credential Offer
    command_handler(&state, &received_offer_id, &state.command.offer, command).await?;

    let command = OfferCommand::SendCredentialRequest {
        received_offer_id: received_offer_id.clone(),
    };

    // Send the Credential Request
    command_handler(&state, &received_offer_id, &state.command.offer, command).await?;

    let credentials = match query_handler(&received_offer_id, &state.query.received_offer).await? {
        Some(ReceivedOfferView { credentials, .. }) => credentials,
        // TODO: this *should* be an impossible error, what should we return here?
        _ => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR)),
    };

    for OfferCredential {
        holder_credential_id,
        credential,
    } in credentials
    {
        let command = CredentialCommand::AddCredential {
            holder_credential_id: holder_credential_id.clone(),
            received_offer_id: Some(received_offer_id.clone()),
            credential,
        };

        // Add the Credential to the state.
        command_handler(&state, &holder_credential_id, &state.command.credential, command).await?;
    }

    query_handler(&received_offer_id, &state.query.received_offer)
        .await?
        .map(|received_offer_view| (StatusCode::CREATED, Json(received_offer_view)).into_response())
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tests::OFFER_ID,
        v0::{
            authorization,
            issuance::{self, credentials::tests::credentials},
        },
        API_VERSION,
    };
    use agent_authorization::services::AuthorizationServices;
    use agent_holder::services::HolderServices;
    use agent_issuance::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use agent_shared::{
        config::{config, config_mut, set_config},
        handlers::command_handler,
    };
    use agent_store::{authorization_state, holder_state, in_memory::InMemory, issuance_state};
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use oid4vci::credential_offer::CredentialOffer;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tower::Service as _;

    async fn bootstrap_credential_offer() -> CredentialOffer {
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let issuer_url = format!("http://{}", listener.local_addr().unwrap());
        let application_url = issuer_url.parse().unwrap();

        config_mut().application_url = application_url;
        config_mut().public_url = issuer_url.parse().unwrap();
        config_mut().token_endpoint = format!("{issuer_url}/auth/token").parse().unwrap();
        config_mut().credential_endpoint = format!("{issuer_url}/openid4vci/credential").parse().unwrap();
        config_mut().credential_offer_uri = format!("{issuer_url}/openid4vci/credential-offer/").parse().unwrap();

        let issuance_state =
            Arc::new(issuance_state(&InMemory, IssuanceServices::default().await, Default::default()).await);
        agent_issuance::state::initialize(&issuance_state).await.unwrap();
        let mut credential_issuer = issuance::router(issuance_state.clone());

        let authorization_state =
            Arc::new(authorization_state(&InMemory, AuthorizationServices::default().await, Default::default()).await);
        agent_authorization::state::initialize(&authorization_state)
            .await
            .unwrap();
        let authorization_server = authorization::router((authorization_state, issuance_state));

        credentials(&mut credential_issuer, "001").await;

        let response = credential_issuer
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/offers"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": OFFER_ID,
                            "credentialConfigurationIds": ["001"],
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let credential_offer: CredentialOffer = String::from_utf8(body.to_vec()).unwrap().parse().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, credential_issuer.merge(authorization_server))
                .await
                .unwrap();
        });

        credential_offer
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn accept_dispatches_offer_and_credential_commands() {
        let original = config().clone();
        let holder_state = Arc::new(holder_state(&InMemory, HolderServices::default().await, Default::default()).await);
        let received_offer_id = "received-offer-id".to_string();

        command_handler(
            &holder_state,
            &received_offer_id,
            &holder_state.command.offer,
            OfferCommand::ReceiveCredentialOffer {
                received_offer_id: received_offer_id.clone(),
                credential_offer: bootstrap_credential_offer().await,
            },
        )
        .await
        .unwrap();

        let response = accept(State(holder_state), Path(received_offer_id)).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        *set_config() = original;
    }
}
