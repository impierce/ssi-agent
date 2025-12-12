pub mod all_managed_keys;

use super::aggregate::ManagedKey;
use cqrs_es::{EventEnvelope, View};

pub type ManagedKeyView = ManagedKey;
impl View<ManagedKey> for ManagedKey {
    fn update(&mut self, event: &EventEnvelope<ManagedKey>) {
        use crate::managed_key::event::ManagedKeyEvent::*;

        // match &event.payload {
        //     _ => todo!(),
        // }
    }
}
