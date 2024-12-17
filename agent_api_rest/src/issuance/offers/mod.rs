pub mod send;

use agent_issuance::{
    offer::{command::OfferCommand, views::OfferView},
    server_config::queries::ServerConfigView,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use agent_shared::handlers::{command_handler, query_handler};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hyper::header;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use tracing::info;
use utoipa::ToSchema;

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OffersEndpointRequest {
    pub offer_id: String,
}

/// Offer a credential
///
/// Create a new offer for one or more credentials.
#[utoipa::path(
    post,
    path = "/offers",
    request_body(content = OffersEndpointRequest, example = json!({"offerId": "0001"})),
    tag = "Issuance",
    responses(
        (status = 200, description = "Successfully created a new credential offer. Response value is standard-compliant and can be interpreted by an identity wallet.", body = String, content_type = "application/x-www-form-urlencoded", example = json!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fcredential-issuer.example.com%2F%22%2C%22credentials%22%3A%5B%7B%22format%22%3A%22ldp_vc%22%2C%22credential_definition%22%3A%7B%22%40context%22%3A%5B%22https%3A%2F%2Fwww.w3.org%2F2018%2Fcredentials%2Fv1%22%2C%22https%3A%2F%2Fwww.w3.org%2F2018%2Fcredentials%2Fexamples%2Fv1%22%5D%2C%22type%22%3A%5B%22VerifiableCredential%22%2C%22UniversityDegreeCredential%22%5D%7D%7D%5D%7D"))
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

/// List all offers
///
/// Retrieve all available credential offers.
#[utoipa::path(
    get,
    path = "/offers",
    tag = "Issuance",
    responses(
        (status = 200, description = "Successfully retrieved all credential offers.", example = json!([{"id":"001"},{"id":"002"}]))
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn all_offers(State(state): State<IssuanceState>) -> Response {
    match query_handler("all_offers", &state.query.all_offers).await {
        Ok(Some(all_offers_view)) => {
            let all_offers = all_offers_view.offers.into_values().collect::<Vec<_>>();

            (StatusCode::OK, Json(all_offers)).into_response()
        }
        Ok(None) => (StatusCode::OK, Json(json!([]))).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Get an offer by ID
///
/// Retrieve an offer by ID.
#[utoipa::path(
    get,
    path = "/offers/{id}",
    params(
        ("id" = String, Path, description = "Unique identifier of the offer", example = "57ea9bf4-3a50-4b34-a340-7ef969bfab12"),
    ),
    tag = "Issuance",
    responses(
        (status = 200, description = "Offer found")
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offer(State(state): State<IssuanceState>, Path(offer_id): Path<String>) -> Response {
    match query_handler(&offer_id, &state.query.offer).await {
        Ok(Some(offer_view)) => (StatusCode::OK, Json(offer_view)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
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
