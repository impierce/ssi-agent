use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query, View};
use serde::{Deserialize, Serialize};

use crate::offer::aggregate::Offer;

pub struct SimpleLoggingQuery {}

#[async_trait]
impl Query<Offer> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Offer>]) {}
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OfferView {
    aggregate_id: Option<String>,
}

impl View<Offer> for OfferView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        println!("TODO: should be updating the view ...");
    }
}
