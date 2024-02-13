use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    credential_request::CredentialRequest,
    token_request::TokenRequest,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    CreateOffer,
    AddCredential {
        credential_ids: Vec<String>,
    },
    // // TODO: add option for credential_offer_uri (by reference)
    CreateCredentialOffer {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },

    // OpenID4VCI Pre-Authorized Code Flow
    CreateTokenResponse {
        token_request: TokenRequest,
    },
    CreateCredentialResponse {
        credential_issuer_metadata: CredentialIssuerMetadata,
        authorization_server_metadata: AuthorizationServerMetadata,
        credentials: Vec<serde_json::Value>,
        credential_request: CredentialRequest,
    },
}
