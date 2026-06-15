use crate::public_offer::{command::PublicOfferCommand, error::PublicOfferError, event::PublicOfferEvent};
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
            PublicOfferCommand::Create { offer_id, template_id } => {
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::services::IssuanceServices;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type PublicOfferTestFramework = TestFramework<PublicOffer>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_public_offer() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        let aggregate = PublicOffer::default();
        let events = aggregate
            .handle(
                PublicOfferCommand::Create {
                    offer_id: offer_id.clone(),
                    template_id: template_id.clone(),
                },
                &issuance_services,
            )
            .await
            .expect("create command should succeed");

        assert_eq!(events.len(), 1);
        match &events[0] {
            PublicOfferEvent::Created {
                offer_id: emitted_offer_id,
                template_id: emitted_template_id,
                created_at,
            } => {
                assert_eq!(emitted_offer_id, &offer_id);
                assert_eq!(emitted_template_id, &template_id);
                assert!(*created_at <= chrono::Utc::now());
            }
            _ => panic!("expected Created event"),
        }
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_duplicate_public_offer_fails() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![PublicOfferEvent::Created {
                offer_id: offer_id.clone(),
                template_id: template_id.clone(),
                created_at: chrono::Utc::now(),
            }])
            .when(PublicOfferCommand::Create { offer_id, template_id })
            .then_expect_error(PublicOfferError::AlreadyExists);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_public_offer_offline() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![PublicOfferEvent::Created {
                offer_id: offer_id.clone(),
                template_id: template_id.clone(),
                created_at: chrono::Utc::now(),
            }])
            .when(PublicOfferCommand::TakeOffline {
                offer_id: offer_id.clone(),
            })
            .then_expect_events(vec![PublicOfferEvent::TakenOffline { offer_id }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_public_offer_offline_idempotent() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![
                PublicOfferEvent::Created {
                    offer_id: offer_id.clone(),
                    template_id: template_id.clone(),
                    created_at: chrono::Utc::now(),
                },
                PublicOfferEvent::TakenOffline {
                    offer_id: offer_id.clone(),
                },
            ])
            .when(PublicOfferCommand::TakeOffline { offer_id })
            .then_expect_events(vec![]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_public_offer_online() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![
                PublicOfferEvent::Created {
                    offer_id: offer_id.clone(),
                    template_id: template_id.clone(),
                    created_at: chrono::Utc::now(),
                },
                PublicOfferEvent::TakenOffline {
                    offer_id: offer_id.clone(),
                },
            ])
            .when(PublicOfferCommand::TakeOnline {
                offer_id: offer_id.clone(),
            })
            .then_expect_events(vec![PublicOfferEvent::TakenOnline { offer_id }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_public_offer_online_idempotent() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![PublicOfferEvent::Created {
                offer_id: offer_id.clone(),
                template_id: template_id.clone(),
                created_at: chrono::Utc::now(),
            }])
            .when(PublicOfferCommand::TakeOnline { offer_id })
            .then_expect_events(vec![]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_public_offer() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![PublicOfferEvent::Created {
                offer_id: offer_id.clone(),
                template_id: template_id.clone(),
                created_at: chrono::Utc::now(),
            }])
            .when(PublicOfferCommand::Delete {
                offer_id: offer_id.clone(),
            })
            .then_expect_events(vec![PublicOfferEvent::Deleted { offer_id }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_delete_nonexistent_public_offer_fails() {
        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given_no_previous_events()
            .when(PublicOfferCommand::Delete {
                offer_id: "nonexistent".to_string(),
            })
            .then_expect_error(PublicOfferError::NotFound);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_offline_nonexistent_public_offer_fails() {
        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given_no_previous_events()
            .when(PublicOfferCommand::TakeOffline {
                offer_id: "nonexistent".to_string(),
            })
            .then_expect_error(PublicOfferError::NotFound);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_take_online_nonexistent_public_offer_fails() {
        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given_no_previous_events()
            .when(PublicOfferCommand::TakeOnline {
                offer_id: "nonexistent".to_string(),
            })
            .then_expect_error(PublicOfferError::NotFound);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_public_offer_state_transitions() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        PublicOfferTestFramework::with(issuance_services)
            .given(vec![PublicOfferEvent::Created {
                offer_id: offer_id.clone(),
                template_id: template_id.clone(),
                created_at: chrono::Utc::now(),
            }])
            .when(PublicOfferCommand::TakeOffline { offer_id })
            .then_expect_events(vec![PublicOfferEvent::TakenOffline {
                offer_id: "public-offer-123".to_string(),
            }]);
    }
}
