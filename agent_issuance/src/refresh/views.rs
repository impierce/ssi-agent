use crate::refresh::aggregate::RefreshCapability;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A refresh capability as it was last recorded, under its token hash. A capability whose continuation was accepted
/// shows as active until the next refresh with its token records the retirement.
pub type RefreshCapabilityView = RefreshCapability;

impl View<RefreshCapability> for RefreshCapability {
    fn update(&mut self, event: &EventEnvelope<RefreshCapability>) {
        cqrs_es::Aggregate::apply(self, event.payload.clone());
    }
}

/// Every refresh capability, by token hash.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AllRefreshCapabilitiesView {
    #[serde(flatten)]
    pub capabilities: HashMap<String, RefreshCapabilityView>,
}

impl View<RefreshCapability> for AllRefreshCapabilitiesView {
    fn update(&mut self, event: &EventEnvelope<RefreshCapability>) {
        self.capabilities
            .entry(event.aggregate_id.clone())
            .or_default()
            .update(event);
    }
}
