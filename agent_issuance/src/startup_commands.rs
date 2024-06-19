use std::collections::HashMap;

use crate::server_config::command::ServerConfigCommand;
use agent_shared::{config, metadata::Metadata, url_utils::UrlAppendHelpers};
use oid4vci::{
    credential_format_profiles::{
        w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
    },
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_configurations_supported::CredentialConfigurationsSupportedObject,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    proof::KeyProofMetadata,
    ProofType,
};
use serde_json::json;

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url, metadata: &Metadata) -> Vec<ServerConfigCommand> {
    vec![
        load_server_metadata(host.clone(), metadata),
        create_credentials_supported(metadata),
    ]
}

pub fn load_server_metadata(base_url: url::Url, metadata: &Metadata) -> ServerConfigCommand {
    let display = metadata.display.first().map(|display| {
        let display = serde_json::to_value(display).unwrap();
        vec![display]
    });

    ServerConfigCommand::InitializeServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: base_url.clone(),
            token_endpoint: Some(base_url.append_path_segment("auth/token")),
            ..Default::default()
        }),
        credential_issuer_metadata: CredentialIssuerMetadata {
            credential_issuer: base_url.clone(),
            credential_endpoint: base_url.append_path_segment("openid4vci/credential"),
            display,
            ..Default::default()
        },
    }
}

// TODO: Should not be a static startup command. Should be dynamic based on the configuration and/or updatable.
pub fn create_credentials_supported(metadata: &Metadata) -> ServerConfigCommand {
    let cryptographic_binding_methods_supported = metadata
        .subject_syntax_types_supported
        .iter()
        .map(ToString::to_string)
        .collect();

    let credential_signing_alg_values_supported = metadata
        .signing_algorithms_supported
        .iter()
        .map(|algorithm| json!(algorithm).as_str().unwrap().to_string())
        .collect();

    ServerConfigCommand::CreateCredentialConfiguration {
        credential_configurations_supported: vec![(
            "badge".to_string(),
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (
                        CredentialDefinition {
                            type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                            credential_subject: Default::default(),
                        },
                        None,
                    )
                        .into(),
                }),
                cryptographic_binding_methods_supported,
                credential_signing_alg_values_supported,
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: metadata.signing_algorithms_supported.clone(),
                    },
                )]),
                display: match (
                    config!("credential_name", String),
                    config!("credential_logo_url", String),
                ) {
                    (Ok(name), Ok(logo_uri)) => vec![json!({
                        "name": name,
                        "logo": {
                            "url": logo_uri
                        }
                    })],
                    _ => vec![],
                },
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
    }
}
