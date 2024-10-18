pub mod all_credentials;

use super::event::CredentialEvent;
use crate::credential::aggregate::Credential;
use cqrs_es::{EventEnvelope, View};

pub type CredentialView = Credential;

impl View<Credential> for Credential {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        match &event.payload {
            CredentialEvent::UnsignedCredentialCreated {
                credential_id,
                data,
                credential_configuration,
            } => {
                self.credential_id.clone_from(credential_id);
                self.data.replace(data.clone());
                self.credential_configuration = credential_configuration.clone();
            }
            CredentialEvent::SignedCredentialCreated {
                credential_id,
                signed_credential,
            } => {
                self.credential_id.clone_from(credential_id);
                self.signed.replace(signed_credential.clone());
            }
            CredentialEvent::CredentialSigned {
                credential_id,
                signed_credential,
                status,
            } => {
                self.credential_id.clone_from(credential_id);
                self.signed.replace(signed_credential.clone());
                self.status.clone_from(status);
            }
        }
    }
}
