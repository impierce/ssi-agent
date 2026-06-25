pub mod all_reissuances;

use super::aggregate::Reissuance;
use super::event::ReissuanceEvent;
use cqrs_es::{EventEnvelope, View};

pub type ReissuanceView = Reissuance;

impl View<Reissuance> for Reissuance {
    fn update(&mut self, event: &EventEnvelope<Reissuance>) {
        match &event.payload {
            ReissuanceEvent::ReissuanceCreated {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
                credential_configuration_id,
                reason,
                trigger_type,
                triggered_by,
                status_action,
                created_at,
            } => {
                self.reissuance_id.clone_from(reissuance_id);
                self.original_credential_id.clone_from(original_credential_id);
                self.new_credential_id.clone_from(new_credential_id);
                self.offer_id.clone_from(offer_id);
                self.credential_configuration_id.clone_from(credential_configuration_id);
                self.reason.clone_from(reason);
                self.trigger_type.clone_from(trigger_type);
                self.triggered_by.clone_from(triggered_by);
                self.status_action.clone_from(status_action);
                self.created_at.replace(*created_at);
            }
        }
    }
}
