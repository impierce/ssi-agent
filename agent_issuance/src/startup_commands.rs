use crate::server_config::command::ServerConfigCommand;
use agent_shared::{config, url_utils::UrlAppendHelpers};
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
use serde_json::json;

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url) -> Vec<ServerConfigCommand> {
    vec![
        // load_credential_format_template(),
        load_authorization_server_metadata(host.clone()),
        load_credential_issuer_metadata(host.clone()),
        create_credentials_supported(),
    ]
}

pub fn load_authorization_server_metadata(base_url: url::Url) -> ServerConfigCommand {
    ServerConfigCommand::LoadAuthorizationServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: base_url.clone(),
            token_endpoint: Some(base_url.append_path_segment("auth/token")),
            ..Default::default()
        }),
    }
}

pub fn load_credential_issuer_metadata(base_url: url::Url) -> ServerConfigCommand {
    ServerConfigCommand::LoadCredentialIssuerMetadata {
        credential_issuer_metadata: CredentialIssuerMetadata {
            credential_issuer: base_url.clone(),
            authorization_server: None,
            credential_endpoint: base_url.append_path_segment("openid4vci/credential"),
            deferred_credential_endpoint: None,
            batch_credential_endpoint: None,
            credentials_supported: vec![],
            display: None,
        },
    }
}

pub fn create_credentials_supported() -> ServerConfigCommand {
    ServerConfigCommand::CreateCredentialsSupported {
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
            display: match (config!("credential_name"), config!("credential_logo_url")) {
                (Ok(name), Ok(logo_url)) => Some(vec![json!({
                    "name": name,
                    "logo": {
                        "url": logo_url
                    }
                })]),
                _ => None,
            },
        }],
    }
}
