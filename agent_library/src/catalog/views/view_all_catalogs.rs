use crate::catalog::aggregate::Catalog;
use crate::catalog::views::CatalogView;
use cqrs_es::{EventEnvelope, View};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllCatalogsView {
    #[serde(flatten)]
    pub catalogs: HashMap<String, CatalogView>,
}

impl View<Catalog> for AllCatalogsView {
    fn update(&mut self, event: &EventEnvelope<Catalog>) {
        let view = self
            .catalogs
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default();
        // update the view with the event
        view.update(event);
        if view.deleted {
            self.catalogs.remove(&event.aggregate_id);
        }
    }
}
