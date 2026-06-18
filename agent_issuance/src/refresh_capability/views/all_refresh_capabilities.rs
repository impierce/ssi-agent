use std::collections::HashMap;

use crate::refresh_capability::aggregate::RefreshCapability;

use super::RefreshCapabilityView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllRefreshCapabilitiesView {
    #[serde(flatten)]
    pub refresh_capabilities: HashMap<String, RefreshCapabilityView>,
}

impl View<RefreshCapability> for AllRefreshCapabilitiesView {
    fn update(&mut self, event: &EventEnvelope<RefreshCapability>) {
        self.refresh_capabilities
            .entry(event.aggregate_id.clone())
            .or_default()
            .update(event);
    }
}
