use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    credential_offer::{GrantType, TxCodeConstraints},
    credential_request::CredentialRequest,
};
use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    CreateCredentialOffer {
        offer_id: String,
        credential_configuration_ids: Vec<String>,
        grant_types: Vec<GrantType>,
        tx_code_constraints: Option<TxCodeConstraints>,
    },
    AddCredentials {
        offer_id: String,
        credential_ids: Vec<String>,
        credential_configuration_ids: Vec<String>,
    },
    SendCredentialOffer {
        offer_id: String,
        target_url: Url,
    },

    // OpenID4VCI Pre-Authorized Code Flow
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
