use super::ReceivedOfferView;
use crate::offer::queries::Offer;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllReceivedOffersView {
    #[serde(flatten)]
    pub received_offers: HashMap<String, ReceivedOfferView>,
}

impl View<Offer> for AllReceivedOffersView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        self.received_offers
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
