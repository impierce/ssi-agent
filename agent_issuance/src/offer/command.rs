use crate::offer::aggregate::{DeliveryMethod, DeliveryOptions};
use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    credential_offer::{GrantType, TxCodeConstraints},
    credential_request::CredentialRequest,
    token_request::TokenRequest,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum OfferCommand {
    CreateCredentialOffer {
        offer_id: String,
        credential_configuration_ids: Vec<String>,
        grant_types: Vec<GrantType>,
        tx_code_constraints: Option<TxCodeConstraints>,
        #[serde(default)]
        delivery_options: Option<DeliveryOptions>,
    },
    AddCredentials {
        offer_id: String,
        credential_ids: Vec<String>,
        credential_configuration_ids: Vec<String>,
    },
    SendCredentialOffer {
        offer_id: String,
        delivery_method: DeliveryMethod,
    },

    // OpenID4VCI Pre-Authorized Code Flow
    CreateTokenResponse {
        offer_id: String,
        token_request: TokenRequest,
    },
    VerifyCredentialRequest {
        offer_id: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credential_request: CredentialRequest,
    },
    CreateCredentialResponse {
        offer_id: String,
        signed_credentials: Vec<(serde_json::Value, Option<String>)>,
    },
}
