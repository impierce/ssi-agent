use agent_shared::config::config;
use agent_shared::url_utils::UrlAppendHelpers;
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};

use crate::server_config::command::ServerConfigCommand;

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url) -> Vec<ServerConfigCommand> {
    vec![load_server_metadata(host), create_credentials_supported()]
}

pub fn load_server_metadata(base_url: url::Url) -> ServerConfigCommand {
    let display = config().display.first().map(|display| {
        let display = serde_json::to_value(display).unwrap();
        vec![display]
    });

    let token_endpoint = config()
        .openid4vci_endpoints
        .token_endpoint
        .clone()
        .or_else(|| Some(base_url.clone().append_path_segment("auth/token")));

    let credential_endpoint = config()
        .openid4vci_endpoints
        .credential_endpoint
        .clone()
        .unwrap_or_else(|| base_url.clone().append_path_segment("openid4vci/credential"));

    ServerConfigCommand::InitializeServerMetadata {
        authorization_server_metadata: Box::new(AuthorizationServerMetadata {
            issuer: base_url.clone(),
            token_endpoint,
            ..Default::default()
        }),
        credential_issuer_metadata: Box::new(CredentialIssuerMetadata {
            credential_issuer: base_url.clone(),
            credential_endpoint,
            display,
            ..Default::default()
        }),
    }
}

pub fn create_credentials_supported() -> ServerConfigCommand {
    let credential_configuration = config()
        .credential_configurations
        .first()
        .expect("No credential_configurations found")
        .clone();

    ServerConfigCommand::AddCredentialConfiguration {
        credential_configuration,
    }
}
