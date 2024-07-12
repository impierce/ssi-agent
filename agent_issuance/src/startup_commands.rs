use agent_shared::config::config_2;
use agent_shared::{metadata::Metadata, url_utils::UrlAppendHelpers};
use oid4vci::credential_issuer::{
    authorization_server_metadata::AuthorizationServerMetadata, credential_issuer_metadata::CredentialIssuerMetadata,
};

use crate::server_config::command::ServerConfigCommand;

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url, metadata: &Metadata) -> Vec<ServerConfigCommand> {
    vec![load_server_metadata(host, metadata), create_credentials_supported()]
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

pub fn create_credentials_supported() -> ServerConfigCommand {
    let credential_configuration = config_2()
        .credential_configurations
        .first()
        .expect("No credential_configurations found")
        .clone();

    ServerConfigCommand::AddCredentialConfiguration {
        credential_configuration,
    }
}
