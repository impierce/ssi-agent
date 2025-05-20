use agent_shared::config::{config, CredentialConfiguration};
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

pub fn load_server_metadata(public_url: url::Url) -> ServerConfigCommand {
    let display = config().display.first().map(|display| {
        let display = serde_json::to_value(display).unwrap();
        vec![display]
    });

    let token_endpoint = config().token_endpoint.clone();
    let credential_endpoint = config().credential_endpoint.clone();

    ServerConfigCommand::InitializeServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: public_url.clone(),
            token_endpoint: Some(token_endpoint),
            ..Default::default()
        }),
        credential_issuer_metadata: Box::new(CredentialIssuerMetadata {
            credential_issuer: public_url.clone(),
            credential_endpoint,
            display,
            ..Default::default()
        }),
    }
}

pub fn create_credentials_supported() -> Vec<ServerConfigCommand> {
    let credential_configurations: Vec<CredentialConfiguration> =
        // TODO: make sure that multiple configurations are supported
        config().credential_configurations.iter().take(1).cloned().collect();

    let mut commands = Vec::new();
    for credential_configuration in credential_configurations {
        commands.push(ServerConfigCommand::AddCredentialConfiguration {
            credential_configuration,
        });
    }
    commands
}
