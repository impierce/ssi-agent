use agent_shared::config;
use lazy_static::lazy_static;
use oid4vci::{
    credential_format_profiles::{
        w3c_verifiable_credentials::jwt_vc_json::{CredentialDefinition, JwtVcJson},
        CredentialFormats, Parameters,
    },
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, credentials_supported::CredentialsSupportedObject,
    },
    ProofType,
};

use crate::command::IssuanceCommand;

lazy_static! {
    static ref BASE_URL: url::Url = format!("http://{}:3033/", config!("host").unwrap()).parse().unwrap();
}

/// Returns the startup commands for the application.
pub fn startup_commands() -> Vec<IssuanceCommand> {
    vec![
        load_credential_format_template(),
        load_authorization_server_metadata(BASE_URL.clone()),
        load_credential_issuer_metadata(BASE_URL.clone()),
        create_credentials_supported(),
    ]
}

pub fn load_credential_format_template() -> IssuanceCommand {
    IssuanceCommand::LoadCredentialFormatTemplate {
        credential_format_template: serde_json::from_str(include_str!(
            "../res/credential_format_templates/openbadges_v3.json"
        ))
        .unwrap(),
    }
}

pub fn load_authorization_server_metadata(base_url: url::Url) -> IssuanceCommand {
    IssuanceCommand::LoadAuthorizationServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: base_url.clone(),
            token_endpoint: Some(base_url.join("auth/token").unwrap()),
            ..Default::default()
        }),
    }
}

pub fn load_credential_issuer_metadata(base_url: url::Url) -> IssuanceCommand {
    IssuanceCommand::LoadCredentialIssuerMetadata {
        credential_issuer_metadata: CredentialIssuerMetadata {
            credential_issuer: base_url.clone(),
            authorization_server: None,
            credential_endpoint: base_url.join("openid4vci/credential").unwrap(),
            deferred_credential_endpoint: None,
            batch_credential_endpoint: None,
            credentials_supported: vec![],
            display: None,
        },
    }
}

pub fn create_credentials_supported() -> IssuanceCommand {
    IssuanceCommand::CreateCredentialsSupported {
        credentials_supported: vec![CredentialsSupportedObject {
            id: None,
            credential_format: CredentialFormats::JwtVcJson(Parameters {
                format: JwtVcJson,
                parameters: (
                    CredentialDefinition {
                        type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                        credential_subject: None,
                    },
                    None,
                )
                    .into(),
            }),
            scope: None,
            cryptographic_binding_methods_supported: Some(vec!["did:key".to_string()]),
            cryptographic_suites_supported: Some(vec!["EdDSA".to_string()]),
            proof_types_supported: Some(vec![ProofType::Jwt]),
            display: None,
        }],
    }
}
