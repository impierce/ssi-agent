use crate::catalogue::aggregate::Catalogue;
use crate::catalogue::views::CatalogueView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllCataloguesView {
    #[serde(flatten)]
    pub catalogues: HashMap<String, CatalogueView>,
}

impl View<Catalogue> for AllCataloguesView {
    fn update(&mut self, event: &EventEnvelope<Catalogue>) {
        self.catalogues
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
