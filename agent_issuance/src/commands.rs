use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    },
    credential_request::CredentialRequest,
    token_request::TokenRequest,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum IssuanceCommand {
    // Image Management
    UploadImage {
        id: String,
        data: String,
    },

    // Initialize Agent
    LoadCredentialFormatTemplate {
        credential_format_template: serde_json::Value,
    },
    LoadAuthorizationServerMetadata {
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    },
    LoadCredentialIssuerMetadata {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },

    // Subject Management
    CreateCredentialsSupported {
        credentials_supported: Vec<CredentialsSupportedObject>,
    },
    CreateUnsignedCredential {
        subject_id: String,
        credential: serde_json::Value,
    },
    // TODO: add option for credential_offer_uri (by reference)
    CreateCredentialOffer {
        subject_id: String,
        pre_authorized_code: Option<String>,
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
