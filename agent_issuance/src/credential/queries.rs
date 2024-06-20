use super::{entity::Data, event::CredentialEvent};
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    pub data: Option<Data>,
    pub credential_configuration: CredentialConfigurationsSupportedObject,
    pub signed: Option<serde_json::Value>,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        match &event.payload {
            CredentialEvent::UnsignedCredentialCreated {
                data,
                credential_configuration,
            } => {
                self.data.replace(data.clone());
                self.credential_configuration = credential_configuration.clone();
            }
            CredentialEvent::SignedCredentialCreated { signed_credential } => {
                self.signed.replace(signed_credential.clone());
            }
            CredentialEvent::CredentialSigned { signed_credential } => {
                self.signed.replace(signed_credential.clone());
            }
        }
    }
}
