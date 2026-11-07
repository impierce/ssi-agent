use crate::handlers::public_command_handler;
use crate::v0::openapi::PROTOCOL_TAG;
use agent_holder::{offer::command::OfferCommand, state::HolderState};
use agent_shared::sensitive::{sensitive, sensitive_debug};
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use http_api_problem::ApiError;
use hyper::StatusCode;
use oid4vci::credential_offer::CredentialOffer;
use serde::Deserialize;
use std::sync::Arc;
use tracing::debug;

/// A credential offer passed by value or by reference. Exactly one of the two parameters is expected.
#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct CredentialOfferQuery {
    /// JSON-encoded credential offer.
    credential_offer: Option<String>,
    /// URL from which the credential offer can be retrieved.
    credential_offer_uri: Option<String>,
}

/// Receive a credential offer
///
/// Receives a credential offer from a credential issuer, as defined by
/// [OpenID4VCI](https://openid.net/specs/openid-4-verifiable-credential-issuance-1_0.html#name-credential-offer-endpoint).
/// `credential_offer` takes precedence when both parameters are present.
#[utoipa::path(
    get,
    path = "/credential_offer",
    operation_id = "credential_offer",
    tags = ["OpenID4VCI", PROTOCOL_TAG],
    params(CredentialOfferQuery),
    responses(
        (status = 200, description = "Credential offer received"),
        (status = 400, description = "Neither parameter is present, or the credential offer is invalid"),
    )
)]
#[axum_macros::debug_handler]
pub(crate) async fn offers_params(
    State(state): State<Arc<HolderState>>,
    Query(payload): Query<CredentialOfferQuery>,
) -> Result<Response, ApiError> {
    let credential_offer_result: Result<CredentialOffer, _> =
        if let Some(credential_offer) = payload.credential_offer {
            format!("openid-credential-offer://?credential_offer={credential_offer}")
        } else if let Some(credential_offer_uri) = payload.credential_offer_uri {
            format!("openid-credential-offer://?credential_offer_uri={credential_offer_uri}")
        } else {
            return Err(ApiError::new(StatusCode::BAD_REQUEST));
        }
        .parse();

    let credential_offer = match credential_offer_result {
        Ok(credential_offer) => credential_offer,
        Err(_) => return Err(ApiError::new(StatusCode::BAD_REQUEST)),
    };

    log_credential_offer(&credential_offer);

    let received_offer_id = uuid::Uuid::new_v4().to_string();

    let command = OfferCommand::ReceiveCredentialOffer {
        received_offer_id: received_offer_id.clone(),
        credential_offer,
    };

    // Add the Credential Offer to the state.
    public_command_handler(&received_offer_id, &state.command.offer, command).await?;

    Ok(StatusCode::OK.into_response())
}

/// Logs what was offered, without the grant codes (`pre-authorized_code`, `issuer_state`) or full URLs: the
/// `credential_offer_uri` works as a bearer reference to the offer, and either URL may carry credentials.
fn log_credential_offer(credential_offer: &CredentialOffer) {
    match credential_offer {
        CredentialOffer::CredentialOfferUri(uri) => {
            debug!(
                host = uri.host_str(),
                credential_offer_uri = sensitive(uri),
                "Received credential offer by reference"
            );
        }
        CredentialOffer::CredentialOffer(parameters) => {
            let grants = parameters.grants.as_ref();
            debug!(
                credential_issuer_host = parameters.credential_issuer.host_str(),
                credential_configuration_ids = ?*parameters.credential_configuration_ids,
                pre_authorized_code_grant = grants.is_some_and(|grants| grants.pre_authorized_code.is_some()),
                authorization_code_grant = grants.is_some_and(|grants| grants.authorization_code.is_some()),
                credential_offer = sensitive_debug(parameters),
                "Received credential offer"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::sensitive::SensitiveValuesLogged;

    fn credential_offer_by_value() -> CredentialOffer {
        format!(
            "openid-credential-offer://?credential_offer={}",
            urlencoding::encode(
                &serde_json::json!({
                    "credential_issuer": "https://issuer.example/tenant?api_key=secret-api-key",
                    "credential_configuration_ids": ["badge"],
                    "grants": {
                        "authorization_code": { "issuer_state": "secret-issuer-state" },
                        "urn:ietf:params:oauth:grant-type:pre-authorized_code": {
                            "pre-authorized_code": "secret-pre-authorized-code"
                        }
                    }
                })
                .to_string()
            )
        )
        .parse()
        .unwrap()
    }

    #[test]
    #[serial_test::serial]
    #[tracing_test::traced_test]
    fn credential_offer_by_value_is_logged_without_grant_codes() {
        log_credential_offer(&credential_offer_by_value());

        assert!(logs_contain("Received credential offer"));
        assert!(logs_contain("issuer.example"));
        assert!(logs_contain("badge"));
        assert!(!logs_contain("secret-api-key"));
        assert!(!logs_contain("secret-pre-authorized-code"));
        assert!(!logs_contain("secret-issuer-state"));
    }

    #[test]
    #[serial_test::serial]
    #[tracing_test::traced_test]
    fn credential_offer_is_logged_in_full_when_sensitive_values_are_logged() {
        let _enabled = SensitiveValuesLogged::enable();

        log_credential_offer(&credential_offer_by_value());

        assert!(logs_contain("secret-pre-authorized-code"));
        assert!(logs_contain("secret-issuer-state"));
    }

    #[test]
    #[serial_test::serial]
    #[tracing_test::traced_test]
    fn credential_offer_by_reference_is_logged_with_host_only() {
        let credential_offer: CredentialOffer =
            "openid-credential-offer://?credential_offer_uri=https%3A%2F%2Fissuer.example%2Foffers%2Fsecret-offer-id"
                .parse()
                .unwrap();

        log_credential_offer(&credential_offer);

        assert!(logs_contain("Received credential offer by reference"));
        assert!(logs_contain("issuer.example"));
        assert!(!logs_contain("secret-offer-id"));
    }
}
