pub mod send;

use crate::handlers::{command_handler, query_handler};
use agent_issuance::{
    offer::command::OfferCommand,
    state::{IssuanceState, SERVER_CONFIG_ID},
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::header;
use oid4vci::credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata;
use serde::{Deserialize, Serialize};

pub(crate) async fn query_credential_issuer_metadata(
    state: &IssuanceState,
) -> Result<CredentialIssuerMetadata, ApiError> {
    // Get the `CredentialIssuerMetadata` from the `ServerConfigView`.
    query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .and_then(|server_config_view| server_config_view.credential_issuer_metadata)
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersEndpointRequest {
    pub offer_id: String,
}

#[axum_macros::debug_handler]
pub(crate) async fn offers(
    State(state): State<IssuanceState>,
    Json(OffersEndpointRequest { offer_id }): Json<OffersEndpointRequest>,
) -> Result<Response, ApiError> {
    // Create an offer if it does not exist yet.
    if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        let credential_issuer_metadata = query_credential_issuer_metadata(&state).await?;

        let command = OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            credential_issuer_metadata: Box::new(credential_issuer_metadata),
        };

        command_handler(&offer_id, &state.command.offer, command).await?
    };

    query_handler(&offer_id, &state.query.offer)
        .await?
        .and_then(|offer_view| offer_view.form_url_encoded_credential_offer)
        .map(|form_url_encoded_credential_offer| {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/x-www-form-urlencoded")],
                form_url_encoded_credential_offer,
            )
                .into_response()
        })
        // TODO: this *should* be an impossible error, what should we return here?
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))
}

#[axum_macros::debug_handler]
pub(crate) async fn all_offers(State(state): State<IssuanceState>) -> Result<Response, ApiError> {
    let all_offers = query_handler("all_offers", &state.query.all_offers)
        .await?
        .map(|all_offers_view| all_offers_view.offers.into_values().collect::<Vec<_>>())
        .unwrap_or_default();

    Ok((StatusCode::OK, Json(all_offers)).into_response())
}

#[axum_macros::debug_handler]
pub(crate) async fn offer(
    State(state): State<IssuanceState>,
    Path(offer_id): Path<String>,
) -> Result<Response, ApiError> {
    query_handler(&offer_id, &state.query.offer)
        .await?
        .map(|offer_view| (StatusCode::OK, Json(offer_view)).into_response())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND))
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
    use agent_shared::config::set_config;
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

    pub async fn offers(app: &mut Router) -> Option<String> {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/offers"))
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

        match CredentialOffer::from_str(&body).unwrap() {
            CredentialOffer::CredentialOffer(credential_offer) => {
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

                Some(pre_authorized_code)
            }
            CredentialOffer::CredentialOfferUri(credential_offer_uri) => {
                assert_eq!(
                    credential_offer_uri,
                    url::Url::parse(&format!("https://example.com/openid4vci/credential-offer/{OFFER_ID}")).unwrap()
                );

                None
            }
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint() {
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let _pre_authorized_code = offers(&mut app).await;
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint_by_reference() {
        set_config().credential_offer_by_value_enabled = false;
        let issuance_state = in_memory::issuance_state(Service::default(), Default::default()).await;
        initialize(&issuance_state, startup_commands(BASE_URL.clone())).await;

        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let _pre_authorized_code = offers(&mut app).await;
        set_config().credential_offer_by_value_enabled = true;
    }
}
