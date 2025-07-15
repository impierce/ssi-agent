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
                notification_id,
            } => {
                self.credential_id.clone_from(credential_id);
                self.data.replace(data.clone());
                self.credential_configuration = *credential_configuration.clone();
                self.notification_id.clone_from(notification_id);
            }
            CredentialEvent::SignedCredentialCreated {
                credential_id,
                signed_credential,
                notification_id,
            } => {
                self.credential_id.clone_from(credential_id);
                self.signed.replace(signed_credential.clone());
                self.notification_id.clone_from(notification_id);
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
            CredentialEvent::NotificationReceived {
                credential_id,
                notification,
            } => {
                self.credential_id.clone_from(credential_id);
                self.holder_notifications.push(notification.clone());
            }
            CredentialEvent::StatusSet { credential_id, status } => {
                self.credential_id.clone_from(credential_id);
                self.status.clone_from(status);
            }
        }
    }
}
