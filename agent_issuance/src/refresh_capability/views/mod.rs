pub mod all_refresh_capabilities;

use super::aggregate::{RefreshCapability, RefreshCapabilityStatus};
use super::event::RefreshCapabilityEvent;
use cqrs_es::{EventEnvelope, View};

pub type RefreshCapabilityView = RefreshCapability;

impl View<RefreshCapability> for RefreshCapability {
    fn update(&mut self, event: &EventEnvelope<RefreshCapability>) {
        match &event.payload {
            RefreshCapabilityEvent::RefreshCapabilityCreated {
                refresh_reference,
                credential_id,
                created_at,
            } => {
                self.refresh_reference.clone_from(refresh_reference);
                self.credential_id.clone_from(credential_id);
                self.created_at.replace(*created_at);
            }
            RefreshCapabilityEvent::RefreshCapabilityDisabled => {
                self.status = RefreshCapabilityStatus::Disabled;
            }
        }
    }
}
