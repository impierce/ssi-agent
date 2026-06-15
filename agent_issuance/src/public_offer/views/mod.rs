use crate::public_offer::aggregate::PublicOffer;
use cqrs_es::View;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Single aggregate view - projects a single PublicOffer from its events
pub type PublicOfferView = PublicOffer;

impl View<PublicOffer> for PublicOfferView {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<PublicOffer>) {
        match &event.payload {
            crate::public_offer::event::PublicOfferEvent::Created {
                offer_id,
                template_id,
                created_at,
            } => {
                self.id = offer_id.clone();
                self.template_id = template_id.clone();
                self.active = true;
                self.deleted = false;
                self.created_at = Some(*created_at);
            }
            crate::public_offer::event::PublicOfferEvent::TakenOffline { .. } => {
                self.active = false;
            }
            crate::public_offer::event::PublicOfferEvent::TakenOnline { .. } => {
                self.active = true;
            }
            crate::public_offer::event::PublicOfferEvent::Deleted { .. } => {
                self.deleted = true;
            }
        }
    }
}

/// All aggregates view - projects all PublicOffers into a HashMap
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllPublicOffersView {
    pub offers: HashMap<String, PublicOfferView>,
}

impl View<PublicOffer> for AllPublicOffersView {
    fn update(&mut self, event: &cqrs_es::EventEnvelope<PublicOffer>) {
        self.offers
            .entry(event.aggregate_id.clone())
            .or_insert_with(PublicOfferView::default)
            .update(event);
    }
}
