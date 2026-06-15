use crate::public_offer::{
    command::PublicOfferCommand, error::PublicOfferError, event::PublicOfferEvent,
};
use crate::services::IssuanceServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// PublicOffer aggregate root - represents a shareable credential offer
/// that multiple holders can claim from a single QR code.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub struct PublicOffer {
    pub id: String,
    pub template_id: String,
    pub active: bool,
    pub deleted: bool,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait]
impl Aggregate for PublicOffer {
    type Command = PublicOfferCommand;
    type Event = PublicOfferEvent;
    type Error = PublicOfferError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "public_offer".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            PublicOfferCommand::Create {
                offer_id,
                template_id,
            } => {
                if !self.id.is_empty() {
                    return Err(PublicOfferError::AlreadyExists);
                }

                Ok(vec![PublicOfferEvent::Created {
                    offer_id,
                    template_id,
                    created_at: chrono::Utc::now(),
                }])
            }

            PublicOfferCommand::TakeOffline { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }
                if !self.active {
                    return Ok(vec![]);
                }

                Ok(vec![PublicOfferEvent::TakenOffline { offer_id }])
            }

            PublicOfferCommand::TakeOnline { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }
                if self.active {
                    return Ok(vec![]);
                }

                Ok(vec![PublicOfferEvent::TakenOnline { offer_id }])
            }

            PublicOfferCommand::Delete { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }

                Ok(vec![PublicOfferEvent::Deleted { offer_id }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            PublicOfferEvent::Created {
                offer_id,
                template_id,
                created_at,
            } => {
                self.id = offer_id;
                self.template_id = template_id;
                self.active = true;
                self.deleted = false;
                self.created_at = Some(created_at);
            }

            PublicOfferEvent::TakenOffline { .. } => {
                self.active = false;
            }

            PublicOfferEvent::TakenOnline { .. } => {
                self.active = true;
            }

            PublicOfferEvent::Deleted { .. } => {
                self.deleted = true;
            }
        }
    }
}
