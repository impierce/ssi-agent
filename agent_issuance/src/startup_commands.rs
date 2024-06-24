use crate::server_config::command::ServerConfigCommand;
use agent_shared::{config, metadata::Metadata, url_utils::UrlAppendHelpers};
use oid4vci::{
    credential_format_profiles::{CredentialFormats, WithParameters},
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
};
use serde::{Deserialize, Serialize};

/// Returns the startup commands for the application.
pub fn startup_commands(host: url::Url, metadata: &Metadata) -> Vec<ServerConfigCommand> {
    vec![
        load_server_metadata(host.clone(), metadata),
        create_credentials_supported(),
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

#[derive(Deserialize, Serialize, Debug)]
pub struct CredentialConfiguration {
    pub credential_configuration_id: String,
    #[serde(flatten)]
    pub credential_format_with_parameters: CredentialFormats<WithParameters>,
    #[serde(default)]
    pub display: Vec<serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ServerConfig {
    pub credential_configurations: Vec<CredentialConfiguration>,
}

pub fn create_credentials_supported() -> ServerConfigCommand {
    let server_config =
        config!("server_config", ServerConfig).expect("Failed due to missing `issuance-config.yml` file");

    let credential_configuration = server_config.credential_configurations.get(0).clone().unwrap();

    ServerConfigCommand::AddCredentialConfiguration {
        credential_configuration_id: credential_configuration.credential_configuration_id.clone(),
        credential_format_with_parameters: credential_configuration.credential_format_with_parameters.clone(),
        display: credential_configuration.display.clone(),
    }
}
