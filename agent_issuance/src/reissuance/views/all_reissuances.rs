use std::collections::HashMap;

use crate::reissuance::aggregate::Reissuance;

use super::ReissuanceView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllReissuancesView {
    #[serde(flatten)]
    pub reissuances: HashMap<String, ReissuanceView>,
}

impl View<Reissuance> for AllReissuancesView {
    fn update(&mut self, event: &EventEnvelope<Reissuance>) {
        self.reissuances
            .entry(event.aggregate_id.clone())
            .or_default()
            .update(event);
    }
}
