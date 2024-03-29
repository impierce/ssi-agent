use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    credential_request::CredentialRequest,
    token_request::TokenRequest,
};
use serde::Deserialize;

use crate::credential::entity::Data;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    CreateCredentialOffer,
    AddCredentials {
        credential_ids: Vec<String>,
    },

    // OpenID4VCI Pre-Authorized Code Flow
    // TODO: add option for credential_offer_uri (by reference)
    CreateFormUrlEncodedCredentialOffer {
        credential_issuer_metadata: CredentialIssuerMetadata,
    },
    CreateTokenResponse {
        token_request: TokenRequest,
    },
    CreateCredentialResponse {
        credential_issuer_metadata: CredentialIssuerMetadata,
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
        credentials: Vec<Data>,
        credential_request: CredentialRequest,
    },
}
