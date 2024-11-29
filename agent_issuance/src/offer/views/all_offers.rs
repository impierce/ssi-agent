use super::OfferView;
use crate::offer::views::Offer;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllOffersView {
    #[serde(flatten)]
    pub offers: HashMap<String, OfferView>,
}

impl View<Offer> for AllOffersView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        self.offers
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
