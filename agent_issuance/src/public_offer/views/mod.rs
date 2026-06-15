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

#[cfg(test)]
pub mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_single_public_offer_view_created() {
        let mut view = PublicOfferView::default();
        let now = Utc::now();
        let event_envelope = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 0,
            payload: crate::public_offer::event::PublicOfferEvent::Created {
                offer_id: "offer-1".to_string(),
                template_id: "template-1".to_string(),
                created_at: now,
            },
            metadata: Default::default(),
        };

        view.update(&event_envelope);

        assert_eq!(view.id, "offer-1");
        assert_eq!(view.template_id, "template-1");
        assert!(view.active);
        assert!(!view.deleted);
        assert_eq!(view.created_at, Some(now));
    }

    #[test]
    fn test_single_public_offer_view_taken_offline() {
        let mut view = PublicOfferView {
            id: "offer-1".to_string(),
            template_id: "template-1".to_string(),
            active: true,
            deleted: false,
            created_at: Some(Utc::now()),
        };

        let event_envelope = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 1,
            payload: crate::public_offer::event::PublicOfferEvent::TakenOffline {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_envelope);

        assert!(!view.active);
        assert!(!view.deleted);
    }

    #[test]
    fn test_single_public_offer_view_taken_online() {
        let mut view = PublicOfferView {
            id: "offer-1".to_string(),
            template_id: "template-1".to_string(),
            active: false,
            deleted: false,
            created_at: Some(Utc::now()),
        };

        let event_envelope = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 2,
            payload: crate::public_offer::event::PublicOfferEvent::TakenOnline {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_envelope);

        assert!(view.active);
        assert!(!view.deleted);
    }

    #[test]
    fn test_single_public_offer_view_deleted() {
        let mut view = PublicOfferView {
            id: "offer-1".to_string(),
            template_id: "template-1".to_string(),
            active: true,
            deleted: false,
            created_at: Some(Utc::now()),
        };

        let event_envelope = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 3,
            payload: crate::public_offer::event::PublicOfferEvent::Deleted {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_envelope);

        assert!(view.deleted);
    }

    #[test]
    fn test_all_public_offers_view_multiple_offers() {
        let mut view = AllPublicOffersView::default();
        let now = Utc::now();

        // First offer created
        let event1 = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 0,
            payload: crate::public_offer::event::PublicOfferEvent::Created {
                offer_id: "offer-1".to_string(),
                template_id: "template-1".to_string(),
                created_at: now,
            },
            metadata: Default::default(),
        };

        view.update(&event1);
        assert_eq!(view.offers.len(), 1);
        assert!(view.offers.contains_key("offer-1"));
        assert!(view.offers["offer-1"].active);

        // Second offer created
        let event2 = cqrs_es::EventEnvelope {
            aggregate_id: "offer-2".to_string(),
            sequence: 0,
            payload: crate::public_offer::event::PublicOfferEvent::Created {
                offer_id: "offer-2".to_string(),
                template_id: "template-2".to_string(),
                created_at: now,
            },
            metadata: Default::default(),
        };

        view.update(&event2);
        assert_eq!(view.offers.len(), 2);
        assert!(view.offers.contains_key("offer-2"));

        // First offer taken offline
        let event3 = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 1,
            payload: crate::public_offer::event::PublicOfferEvent::TakenOffline {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event3);
        assert!(!view.offers["offer-1"].active);
        assert!(view.offers["offer-2"].active);
    }

    #[test]
    fn test_all_public_offers_view_state_progression() {
        let mut view = AllPublicOffersView::default();
        let now = Utc::now();

        // Create offer
        let event_created = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 0,
            payload: crate::public_offer::event::PublicOfferEvent::Created {
                offer_id: "offer-1".to_string(),
                template_id: "template-1".to_string(),
                created_at: now,
            },
            metadata: Default::default(),
        };

        view.update(&event_created);
        let offer = &view.offers["offer-1"];
        assert!(offer.active);
        assert!(!offer.deleted);

        // Take offline
        let event_offline = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 1,
            payload: crate::public_offer::event::PublicOfferEvent::TakenOffline {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_offline);
        let offer = &view.offers["offer-1"];
        assert!(!offer.active);
        assert!(!offer.deleted);

        // Take online again
        let event_online = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 2,
            payload: crate::public_offer::event::PublicOfferEvent::TakenOnline {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_online);
        let offer = &view.offers["offer-1"];
        assert!(offer.active);
        assert!(!offer.deleted);

        // Delete
        let event_deleted = cqrs_es::EventEnvelope {
            aggregate_id: "offer-1".to_string(),
            sequence: 3,
            payload: crate::public_offer::event::PublicOfferEvent::Deleted {
                offer_id: "offer-1".to_string(),
            },
            metadata: Default::default(),
        };

        view.update(&event_deleted);
        let offer = &view.offers["offer-1"];
        assert!(offer.deleted);
    }
}
