use crate::public_offer::{command::PublicOfferCommand, error::PublicOfferError, event::PublicOfferEvent};
use crate::services::IssuanceServices;
use cqrs_es::{event_sink::EventSink, Aggregate};
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

impl Aggregate for PublicOffer {
    type Command = PublicOfferCommand;
    type Event = PublicOfferEvent;
    type Error = PublicOfferError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "public_offer";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        match command {
            PublicOfferCommand::Create { offer_id, template_id } => {
                if !self.id.is_empty() && !self.deleted {
                    return Err(PublicOfferError::AlreadyExists);
                }

                sink.write(
                    PublicOfferEvent::Created {
                        offer_id,
                        template_id,
                        created_at: chrono::Utc::now(),
                    },
                    self,
                )
                .await;
                Ok(())
            }
            PublicOfferCommand::TakeOffline { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }
                if !self.active {
                    return Ok(());
                }

                sink.write(PublicOfferEvent::TakenOffline { offer_id }, self).await;
                Ok(())
            }
            PublicOfferCommand::TakeOnline { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }
                if self.active {
                    return Ok(());
                }

                sink.write(PublicOfferEvent::TakenOnline { offer_id }, self).await;
                Ok(())
            }
            PublicOfferCommand::Delete { offer_id } => {
                if self.id.is_empty() {
                    return Err(PublicOfferError::NotFound);
                }

                sink.write(PublicOfferEvent::Deleted { offer_id }, self).await;
                Ok(())
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
    use cqrs_es::{event_sink::EventSink, test::TestFramework};
    use rstest::rstest;

    type PublicOfferTestFramework = TestFramework<PublicOffer>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_public_offer() {
        let offer_id = "public-offer-123".to_string();
        let template_id = "template-456".to_string();

        let issuance_services = IssuanceServices::default().await;

        let mut aggregate = PublicOffer::default();
        let sink = EventSink::default();
        aggregate
            .handle(
                PublicOfferCommand::Create {
                    offer_id: offer_id.clone(),
                    template_id: template_id.clone(),
                },
                &issuance_services,
                &sink,
            )
            .await
            .expect("create command should succeed");
        let events = sink.collect().await;

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
    async fn test_recreate_public_offer_after_delete_succeeds() {
        let offer_id = "public-offer-123".to_string();
        let original_template_id = "template-456".to_string();
        let new_template_id = "template-789".to_string();

        let issuance_services = IssuanceServices::default().await;

        let mut aggregate = PublicOffer::default();
        aggregate.apply(PublicOfferEvent::Created {
            offer_id: offer_id.clone(),
            template_id: original_template_id,
            created_at: chrono::Utc::now(),
        });
        aggregate.apply(PublicOfferEvent::Deleted {
            offer_id: offer_id.clone(),
        });

        let sink = EventSink::default();
        aggregate
            .handle(
                PublicOfferCommand::Create {
                    offer_id: offer_id.clone(),
                    template_id: new_template_id.clone(),
                },
                &issuance_services,
                &sink,
            )
            .await
            .expect("recreate command should succeed after delete");
        let events = sink.collect().await;

        assert_eq!(events.len(), 1);
        match &events[0] {
            PublicOfferEvent::Created {
                offer_id: emitted_offer_id,
                template_id: emitted_template_id,
                created_at,
            } => {
                assert_eq!(emitted_offer_id, &offer_id);
                assert_eq!(emitted_template_id, &new_template_id);
                assert!(*created_at <= chrono::Utc::now());
            }
            _ => panic!("expected Created event"),
        }
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
