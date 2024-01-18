use oid4vci::{credential_request::CredentialRequest, token_request::TokenRequest};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    // OpenID4VCI Pre-Authorized Code Flow
    CreateTokenResponse {
        token_request: TokenRequest,
    },
    CreateCredentialResponse {
        access_token: String,
        credential_request: CredentialRequest,
    },
    // TODO: add option for credential_offer_uri (by reference)
    CreateCredentialOffer {
        subject_id: String,
        pre_authorized_code: Option<String>,
    },
}
