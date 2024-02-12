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
        credential_id: String,
    },
    // // TODO: add option for credential_offer_uri (by reference)
    CreateCredentialOffer {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CreateTokenResponse {
        token_request: TokenRequest,
    },
    CreateCredentialResponse {
        credential_issuer_metadata: CredentialIssuerMetadata,
        authorization_server_metadata: AuthorizationServerMetadata,
        credential: serde_json::Value,
        credential_request: CredentialRequest,
    },
    // // OpenID4VCI Pre-Authorized Code Flow
    // CreateTokenResponse {
    //     token_request: TokenRequest,
    // },
    // CreateCredentialResponse {
    //     access_token: String,
    //     credential_request: CredentialRequest,
    // },
    // CreateCredentialOffer {
    //     subject_id: String,
    //     pre_authorized_code: Option<String>,
    // },
}
