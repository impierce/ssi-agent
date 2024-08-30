pub mod send;

use agent_issuance::{
    offer::{command::OfferCommand, queries::OfferView},
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hyper::header;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;
use utoipa::ToSchema;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OffersEndpointRequest {
    pub offer_id: String,
}

/// Create a new offer
///
/// Create a new credential offer for the given ID.
#[utoipa::path(
    post,
    path = "/offers",
    request_body = OffersEndpointRequest,
    tag = "Offers",
    responses(
        (status = 200, description = "Successfully created a new credential offer.")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offers(State(state): State<IssuanceState>, Json(payload): Json<Value>) -> Response {
    info!("Request Body: {}", payload);

    let Ok(OffersEndpointRequest { offer_id }) = serde_json::from_value(payload) else {
        return (StatusCode::BAD_REQUEST, "invalid payload").into_response();
    };

    // Get the `CredentialIssuerMetadata` from the `ServerConfigView`.
    let credential_issuer_metadata = match query_handler(SERVER_CONFIG_ID, &state.query.server_config).await {
        Ok(Some(ServerConfigView {
            credential_issuer_metadata: Some(credential_issuer_metadata),
            ..
        })) => Box::new(credential_issuer_metadata),
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Create an offer if it does not exist yet.
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(_)) => {}
        _ => {
            if command_handler(
                &offer_id,
                &state.command.offer,
                OfferCommand::CreateCredentialOffer {
                    offer_id: offer_id.clone(),
                    credential_issuer_metadata,
                },
            )
            .await
            .is_err()
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    let command = OfferCommand::CreateFormUrlEncodedCredentialOffer {
        offer_id: offer_id.clone(),
    };

    if command_handler(&offer_id, &state.command.offer, command).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(OfferView {
            form_url_encoded_credential_offer,
            ..
        })) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-www-form-urlencoded")],
            form_url_encoded_credential_offer,
        )
            .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::API_VERSION;
    use crate::{
        issuance::{credentials::tests::credentials, router},
        tests::{BASE_URL, OFFER_ID},
    };
    use agent_issuance::{startup_commands::startup_commands, state::initialize};
    use agent_secret_manager::service::Service;
    use agent_store::in_memory;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode};
    use serde_json::json;
    use std::str::FromStr;
    use tower::Service as _;

    pub async fn offers(app: &mut Router) -> String {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(&format!("{API_VERSION}/offers"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": OFFER_ID
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "application/x-www-form-urlencoded"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: String = String::from_utf8(body.to_vec()).unwrap();

        if let CredentialOffer::CredentialOffer(credential_offer) = CredentialOffer::from_str(&body).unwrap() {
            let CredentialOfferParameters {
                grants:
                    Some(Grants {
                        pre_authorized_code:
                            Some(PreAuthorizedCode {
                                pre_authorized_code, ..
                            }),
                        ..
                    }),
                ..
            } = *credential_offer
            else {
                unreachable!()
            };
            pre_authorized_code
        } else {
            unreachable!()
        }
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let _pre_authorized_code = offers(&mut app).await;
    }
}
