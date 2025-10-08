pub mod send;

use crate::{
    error::type_url,
    handlers::{command_handler, query_handler},
};
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
use oid4vci::credential_offer::GrantType;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffersEndpointRequest {
    pub offer_id: String,
    #[serde(default)]
    pub credential_configuration_ids: Vec<String>,
    #[serde(default = "true_as_default")]
    pub is_pre_authorized: bool,
}

fn true_as_default() -> bool {
    true
}

#[axum_macros::debug_handler]
pub(crate) async fn offers(
    State(state): State<IssuanceState>,
    Json(OffersEndpointRequest {
        offer_id,
        credential_configuration_ids,
        is_pre_authorized,
    }): Json<OffersEndpointRequest>,
) -> Result<Response, ApiError> {
    // Check if the credential configuration IDs are valid.
    let credential_configurations = query_handler(SERVER_CONFIG_ID, &state.query.server_config)
        .await?
        .map(|server_config_view| server_config_view.credential_configurations)
        // Unreachable error
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR))?;

    let persisted_credential_configuration_ids = credential_configurations.keys().collect::<Vec<_>>();

    if let Some(credential_configuration_id) = credential_configuration_ids.iter().find(|credential_configuration_id| {
        !persisted_credential_configuration_ids.contains(credential_configuration_id)
    }) {
        return Err(ApiError::builder(StatusCode::NOT_FOUND)
            .title("No Credential Configuration Found")
            .type_url(type_url("issuance#no-credential-configuration-found"))
            .message(format!(
                "No Credential Configuration found with id: `{credential_configuration_id}`"
            ))
            .finish());
    }

    let grant_types = if is_pre_authorized {
        vec![GrantType::PreAuthorizedCode]
    } else {
        vec![GrantType::AuthorizationCode]
    };

    let tx_code_constraints = credential_configuration_ids
        .iter()
        .find_map(|credential_configuration_id| {
            credential_configurations
                .get(credential_configuration_id)
                // TODO: find a way to bind the `authorization` to the Offer somehow
                .and_then(|(_, _, authorization)| {
                    if authorization.pre_authorized {
                        authorization.tx_code_constraints.clone()
                    } else {
                        None
                    }
                })
        });

    // Create an offer if it does not exist yet.
    let command = if query_handler(&offer_id, &state.query.offer).await?.is_none() {
        OfferCommand::CreateCredentialOffer {
            offer_id: offer_id.clone(),
            credential_configuration_ids,
            grant_types: vec![GrantType::PreAuthorizedCode],
            tx_code_constraints,
        }
    } else {
        OfferCommand::UpdateGrantTypes {
            offer_id: offer_id.clone(),
            grant_types,
        }
    };

    command_handler(&offer_id, &state.command.offer, command).await?;

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
        // Unreachable error
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
        tests::OFFER_ID,
    };
    use agent_issuance::state::initialize;
    use agent_secret_manager::service::Service;
    use agent_shared::config::set_config;
    use agent_store::in_memory::InMemory;
    use agent_store::issuance_state;
    use axum::{
        body::Body,
        http::{self, Request},
        Router,
    };
    use oid4vci::credential_offer::{
        AuthorizationCode, CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode,
    };
    use serde_json::json;
    use std::str::FromStr;
    use tower::Service as _;

    pub async fn offers(
        app: &mut Router,
        is_pre_authorized: bool,
    ) -> Option<(Option<AuthorizationCode>, Option<PreAuthorizedCode>)> {
        let response = app
            .call(
                Request::builder()
                    .method(http::Method::POST)
                    .uri(format!("{API_VERSION}/offers"))
                    .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                    .body(Body::from(
                        serde_json::to_vec(&json!({
                            "offerId": OFFER_ID,
                            "credentialConfigurationIds": ["001"],
                            "isPreAuthorized": is_pre_authorized,
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
                assert_eq!(credential_offer.credential_configuration_ids, vec!["001".to_string()]);

                let CredentialOfferParameters {
                    grants:
                        Some(Grants {
                            authorization_code,
                            pre_authorized_code,
                        }),
                    ..
                } = *credential_offer
                else {
                    unreachable!()
                };

                Some((authorization_code, pre_authorized_code))
            }
            CredentialOffer::CredentialOfferUri(credential_offer_uri) => {
                assert_eq!(
                    credential_offer_uri,
                    url::Url::parse(&format!(
                        "https://my-domain.example.org/openid4vci/credential-offer/{OFFER_ID}"
                    ))
                    .unwrap()
                );

                None
            }
        }
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint() {
        let issuance_state = issuance_state(&InMemory, Service::default(), Default::default()).await;
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let (_authorization_code, _pre_authorized_code) = offers(&mut app, true).await.unwrap();
    }

    #[serial_test::serial]
    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_offers_endpoint_by_reference() {
        set_config().credential_offer_by_value_enabled = false;
        let issuance_state = issuance_state(&InMemory, Service::default(), Default::default()).await;
        initialize(&issuance_state).await.unwrap();

        let mut app = router(issuance_state);

        credentials(&mut app).await;
        let none = offers(&mut app, true).await;

        // When `credential_offer_by_value_enabled` is false, we expect no grants to be returned from the `offers` test function.
        assert!(none.is_none());

        set_config().credential_offer_by_value_enabled = true;
    }
}
