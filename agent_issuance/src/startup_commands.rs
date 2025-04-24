use agent_shared::config::{config, CredentialConfiguration};
use agent_shared::url_utils::UrlAppendHelpers;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};

use crate::server_config::command::ServerConfigCommand;

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url) -> Vec<ServerConfigCommand> {
    let mut commands = vec![load_server_metadata(host)];

    commands.extend(create_credentials_supported());

    commands
}

pub fn load_server_metadata(base_url: url::Url) -> ServerConfigCommand {
    let display = config().display.first().map(|display| {
        let display = serde_json::to_value(display).unwrap();
        vec![display]
    });

    ServerConfigCommand::InitializeServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: base_url.clone(),
            token_endpoint: Some(base_url.append_path_segment("auth/token")),
            ..Default::default()
        }),
        credential_issuer_metadata: Box::new(CredentialIssuerMetadata {
            credential_issuer: base_url.clone(),
            credential_endpoint: base_url.append_path_segment("openid4vci/credential"),
            display,
            ..Default::default()
        }),
    }
}

pub fn create_credentials_supported() -> Vec<ServerConfigCommand> {
    let credential_configurations: Vec<CredentialConfiguration> = config()
        .credential_configuration_file
        .as_ref()
        .map(|file| {
            let file = std::fs::read_to_string(file).unwrap();
            serde_json::from_str(&file).unwrap()
        })
        .unwrap_or_else(|| {
            panic!("Credential configuration file not found. Please provide a valid path.");
        });

    let mut commands = Vec::new();
    for credential_configuration in credential_configurations {
        commands.push(ServerConfigCommand::AddCredentialConfiguration {
            credential_configuration,
        });
    }
    commands
}
