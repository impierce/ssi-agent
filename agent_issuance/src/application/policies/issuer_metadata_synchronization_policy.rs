use crate::state::IssuanceState;
use agent_library::template::aggregate::Template;
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use std::sync::Arc;

pub struct IssuerMetadataSynchronizationPolicy {
    // TODO: Actually use this.
    _issuance_state: Arc<IssuanceState>,
}

impl IssuerMetadataSynchronizationPolicy {
    pub fn new(issuance_state: Arc<IssuanceState>) -> Self {
        Self {
            _issuance_state: issuance_state,
        }
    }
}

#[async_trait]
impl Query<Template> for IssuerMetadataSynchronizationPolicy {
    async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<Template>]) {
        use agent_library::template::event::TemplateEvent::*;

        for event in events {
            // TODO: Remove this when we implement the actual issuer metadata synchronization policy.
            #[allow(clippy::single_match)]
            match &event.payload {
                TemplateCreated {
                    title: Some(_title), ..
                } => {
                    // TODO: Update issuer metadata.
                }
                _ => {}
            }
        }
    }
}
