use crate::state::{IssuanceState, SERVER_CONFIG_ID};
use agent_library::template::aggregate::Template;
use agent_shared::handlers::command_handler;
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use oid4vci::{
    credential_format_profiles::{
        w3c_verifiable_credentials::jwt_vc_json::{self, JwtVcJsonParameters},
        CredentialFormats, Parameters,
    },
    credential_issuer::credential_configurations_supported::{CredentialConfigurationsSupportedDisplay, Logo},
};
use std::sync::Arc;

pub struct IssuerMetadataSynchronizationPolicy {
    issuance_state: Arc<IssuanceState>,
}

impl IssuerMetadataSynchronizationPolicy {
    pub fn new(issuance_state: Arc<IssuanceState>) -> Self {
        Self { issuance_state }
    }
}

#[async_trait]
impl Query<Template> for IssuerMetadataSynchronizationPolicy {
    async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<Template>]) {
        use crate::server_config::command::ServerConfigCommand::*;
        use agent_library::template::event::TemplateEvent::*;

        for event in events {
            match &event.payload {
                TemplateCreated {
                    template_id,
                    title,
                    display,
                    authorization,
                    ..
                } => {
                    let new_display = if let Some(display) = display {
                        let old_logo = display.logo.as_ref().unwrap().clone();
                        let logo = Logo {
                            uri: old_logo.uri.parse().unwrap(),
                            alt_text: old_logo.alt_text,
                        };

                        let new_display = CredentialConfigurationsSupportedDisplay {
                            name: display.name.clone(),
                            locale: None,
                            logo: Some(logo),
                            description: None,
                            background_image: None,
                            background_color: None,
                            text_color: None,
                        };

                        Some(new_display)
                    } else {
                        None
                    };

                    let command = CreateCredentialConfiguration {
                        template_id: template_id.clone(),
                        credential_configuration_id: title.clone(),
                        credential_format_with_parameters: CredentialFormats::JwtVcJson(Parameters {
                            parameters: JwtVcJsonParameters {
                                credential_definition: jwt_vc_json::CredentialDefinition {
                                    type_: vec!["VerifiableCredential".to_string()],
                                    credential_subject: Default::default(),
                                },
                            },
                        }),
                        display: if let Some(display) = new_display {
                            vec![display]
                        } else {
                            vec![]
                        },
                        claims: vec![],
                        authorization: authorization.clone(),
                    };

                    let _ =
                        command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await;
                }
                TitleUpdated { template_id, title, .. } => {
                    let command = UpdateCredentialConfigurationId {
                        template_id: template_id.clone(),
                        credential_configuration_id: title.clone(),
                    };

                    let _ =
                        command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await;
                }
                DisplayUpdated {
                    template_id, display, ..
                } => {
                    let logo = if let Some(logo) = &display.logo {
                        Some(Logo {
                            uri: logo.uri.parse().unwrap(),
                            alt_text: logo.alt_text.clone(),
                        })
                    } else {
                        None
                    };

                    let new_display = CredentialConfigurationsSupportedDisplay {
                        name: display.name.clone(),
                        locale: None,
                        logo,
                        description: None,
                        background_image: None,
                        background_color: None,
                        text_color: None,
                    };

                    let command = UpdateCredentialConfigurationDisplay {
                        template_id: template_id.clone(),
                        display: new_display,
                    };

                    let _ =
                        command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await;
                }
                AuthorizationUpdated {
                    template_id,
                    authorization,
                    ..
                } => {
                    let command = UpdateCredentialConfigurationAuthorization {
                        template_id: template_id.clone(),
                        authorization: authorization.clone(),
                    };

                    let _ =
                        command_handler(SERVER_CONFIG_ID, &self.issuance_state.command.server_config, command).await;
                }
                _ => {}
            }
        }
    }
}
