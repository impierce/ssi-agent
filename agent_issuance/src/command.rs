use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    },
    credential_request::CredentialRequest,
    token_request::TokenRequest,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum IssuanceCommand {
    // Initialize Agent
    LoadCredentialFormatTemplate {
        credential_format_template: serde_json::Value,
    },
    LoadAuthorizationServerMetadata {
        authorization_server_metadata: AuthorizationServerMetadata,
    },
    LoadCredentialIssuerMetadata {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },

    // Subject Management
    CreateSubject {
        pre_authorized_code: String,
    },
    CreateCredentialsSupported {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
    // TODO: add option for credential_offer_uri (by reference)
    CreateCredentialOffer,
    CreateUnsignedCredential {
        credential_subject: serde_json::Value,
    },

    // OpenID4VCI Pre-Authorized Code Flow
    CreateTokenResponse {
        token_request: TokenRequest,
    },
    CreateCredentialResponse {
        access_token: String,
        credential_request: CredentialRequest,
    },
}
