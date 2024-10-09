pub mod all_credentials;

use super::event::CredentialEvent;
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};

pub type CredentialView = Credential;

impl View<Credential> for Credential {
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
            CredentialEvent::CredentialSigned {
                signed_credential,
                status,
            } => {
                self.signed.replace(signed_credential.clone());
                self.status.clone_from(status);
            }
        }
    }
}
