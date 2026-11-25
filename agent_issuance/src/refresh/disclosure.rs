//! A refresh token leaves UniCore only inside the credential delivered to its holder.
//!
//! The credential's own events and views keep the token: the signed credential is stored for delivery and can't be
//! changed without breaking its signature. Everything that leaves the store goes through this module first:
//! published events ([`RefreshTokenDisclosure`], installed on the event bus) and the management API's credentials
//! and offers ([`disclosed_credential`], [`disclosed_offer`]).
//!
//! The rule fails closed, whatever the `refreshService.url`: from credential content, `refreshService.refreshToken`
//! is removed whenever it is present. A signed credential is withheld whole when it carries a token: a JWT whose
//! payload has `refreshService.refreshToken` at its root or under `vc`, or an SD-JWT whose issuer-signed payload does,
//! or one of whose disclosures is named `refreshService` and has a `refreshToken`. Everything else passes unchanged.

use crate::{credential::aggregate::Credential, offer::aggregate::Offer};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseType};
use serde_json::Value;
use shared_kernel::event_bus::{CloudEvent, EventDisclosure};

/// The credential as the management API shows it: without the refresh token in its content, and without its signed
/// credential (`null`) when that carries one.
pub fn disclosed_credential(mut credential: Credential) -> Credential {
    if let Some(data) = credential.data.as_mut() {
        withhold_from_content(&mut data.raw);
    }
    credential.signed = credential.signed.filter(|signed| !signed_carries_token(signed));
    credential
}

/// The offer as the management API shows it: without its credential response (`null`) when a credential in it
/// carries a refresh token.
pub fn disclosed_offer(mut offer: Offer) -> Offer {
    offer.credential_response = offer
        .credential_response
        .filter(|credential_response| !response_carries_token(credential_response));
    offer
}

/// The event bus's [`EventDisclosure`] for issuance: removes refresh tokens from credential and offer events, from
/// the content of `UnsignedCredentialCreated`, and the whole `signed_credential` or `credential_response` (`null`)
/// when it carries one. Events of other aggregates pass unchanged.
pub struct RefreshTokenDisclosure;

impl EventDisclosure for RefreshTokenDisclosure {
    fn disclose(&self, event: &mut CloudEvent) {
        if !matches!(event.source.as_str(), "/services/credential" | "/services/offer") {
            return;
        }
        let Some(data) = event.data.as_mut() else {
            return;
        };
        if let Some(content) = data.pointer_mut("/data/raw") {
            withhold_from_content(content);
        }
        if let Some(signed) = data.get_mut("signed_credential") {
            if signed_carries_token(signed) {
                *signed = Value::Null;
            }
        }
        if let Some(credential_response) = data.get_mut("credential_response") {
            let carries = serde_json::from_value::<CredentialResponse>(credential_response.clone())
                .is_ok_and(|credential_response| response_carries_token(&credential_response));
            if carries {
                *credential_response = Value::Null;
            }
        }
    }
}

fn withhold_from_content(content: &mut Value) {
    if let Some(refresh_service) = content.get_mut("refreshService").and_then(Value::as_object_mut) {
        refresh_service.remove("refreshToken");
    }
}

fn response_carries_token(credential_response: &CredentialResponse) -> bool {
    match &credential_response.credential {
        CredentialResponseType::Immediate { credentials, .. } => credentials
            .iter()
            .any(|credential| compact_carries_token(&credential.credential)),
        CredentialResponseType::Deferred { .. } => false,
    }
}

fn signed_carries_token(signed: &Value) -> bool {
    signed.as_str().is_some_and(compact_carries_token)
}

/// A JWT, or an SD-JWT: the issuer-signed JWT, then its disclosures, separated by `~`.
fn compact_carries_token(compact: &str) -> bool {
    let mut parts = compact.split('~');
    let issuer_signed = parts.next().unwrap_or_default();
    let payload = issuer_signed.split('.').nth(1).and_then(decoded_json);
    payload.is_some_and(|payload| payload_carries_token(&payload))
        || parts
            .filter_map(decoded_json)
            .any(|disclosure| disclosure_carries_token(&disclosure))
}

fn payload_carries_token(payload: &Value) -> bool {
    [payload, &payload["vc"]]
        .into_iter()
        .any(|claims| has_refresh_token(&claims["refreshService"]))
}

/// An object property disclosure is `[salt, name, value]`.
fn disclosure_carries_token(disclosure: &Value) -> bool {
    disclosure[1] == "refreshService" && has_refresh_token(&disclosure[2])
}

fn has_refresh_token(refresh_service: &Value) -> bool {
    refresh_service.get("refreshToken").is_some()
}

fn decoded_json(segment: &str) -> Option<Value> {
    let bytes = URL_SAFE_NO_PAD.decode(segment).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oid4vci::credential_response::CredentialResponseObject;
    use serde_json::json;

    fn jwt_with(payload: &Value) -> String {
        format!(
            "e30.{}.c2lnbmF0dXJl",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap())
        )
    }

    fn offer_delivering(credential: String) -> Offer {
        Offer {
            credential_response: Some(CredentialResponse {
                credential: CredentialResponseType::Immediate {
                    credentials: vec![CredentialResponseObject { credential }],
                    notification_id: None,
                },
            }),
            ..Default::default()
        }
    }

    fn signed_with(signed: String) -> Credential {
        Credential {
            signed: Some(json!(signed)),
            ..Default::default()
        }
    }

    #[test]
    fn an_offer_that_delivered_a_refreshable_credential_shows_no_credential_response() {
        let refreshable = jwt_with(&json!({
            "refreshService": {"type": "t", "url": "https://unicore.example/v0/refresh-credential", "refreshToken": "secret"}
        }));

        let offer = disclosed_offer(offer_delivering(refreshable));

        assert_eq!(offer.credential_response, None);
    }

    #[test]
    fn the_token_is_withheld_whatever_the_refresh_service_url() {
        let content = json!({
            "refreshService": {"type": "t", "url": "https://previous.example/v0/refresh-credential", "refreshToken": "secret"}
        });
        let credential = Credential {
            data: Some(crate::credential::entity::Data { raw: content.clone() }),
            signed: Some(json!(jwt_with(&content))),
            ..Default::default()
        };

        let credential = disclosed_credential(credential);

        assert_eq!(
            credential,
            Credential {
                data: Some(crate::credential::entity::Data {
                    raw: json!({"refreshService": {"type": "t", "url": "https://previous.example/v0/refresh-credential"}})
                }),
                signed: None,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_jwt_carrying_the_token_under_vc_is_withheld() {
        let signed = jwt_with(&json!({"vc": {"refreshService": {"refreshToken": "secret"}}}));

        assert_eq!(disclosed_credential(signed_with(signed)).signed, None);
    }

    #[test]
    fn an_sd_jwt_disclosing_the_refresh_service_with_its_token_is_withheld() {
        let disclosure = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&json!(["salt", "refreshService", {"refreshToken": "secret"}])).unwrap());
        let signed = format!("{}~{disclosure}~", jwt_with(&json!({"_sd": ["digest"]})));

        assert_eq!(disclosed_credential(signed_with(signed)).signed, None);
    }

    #[test]
    fn a_credential_without_a_refresh_token_passes_unchanged() {
        let content = json!({
            "first_name": "Ferris",
            "refreshService": {"type": "t", "url": "https://unicore.example/v0/refresh-credential"}
        });
        let credential = Credential {
            data: Some(crate::credential::entity::Data { raw: content.clone() }),
            signed: Some(json!(jwt_with(&content))),
            ..Default::default()
        };

        assert_eq!(disclosed_credential(credential.clone()), credential);
    }
}
