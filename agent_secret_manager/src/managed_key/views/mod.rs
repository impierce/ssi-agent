pub mod all_managed_keys;

use super::aggregate::ManagedKey;
use cqrs_es::{EventEnvelope, View};

pub type ManagedKeyView = ManagedKey;
impl View<ManagedKey> for ManagedKey {
    fn update(&mut self, event: &EventEnvelope<ManagedKey>) {
        use crate::managed_key::event::ManagedKeyEvent::*;

        match &event.payload {
            KeyGenerated {
                managed_key_id,
                key_id,
                alias,
                signing_algorithm,
            } => {
                self.managed_key_id.clone_from(managed_key_id);
                self.key_id.clone_from(key_id);
                self.alias.clone_from(alias);
                self.signing_algorithm.replace(signing_algorithm.clone());
            }
            KeyRemoved { managed_key_id: _ } => {
                // Do not reset the entire state so that the removal can be undone if needed?
                // *self = Self::default();
                self.is_removed = true;
            }
            KeyAliasUpdated {
                managed_key_id: _,
                new_alias,
            } => {
                self.alias.clone_from(new_alias);
            }
            SigningKeySet { managed_key_id: _ } => {
                self.is_signing_key = true;
            }
        }
    }
}
