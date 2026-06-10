use crate::nonce::command::NonceCommand;
use crate::nonce::error::NonceError;
use crate::nonce::event::NonceEvent;
use crate::services::IssuanceServices;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Nonce {
    pub c_nonce: String,
    pub is_redeemed: bool,
}

impl Aggregate for Nonce {
    type Command = NonceCommand;
    type Event = NonceEvent;
    type Error = NonceError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "nonce";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &cqrs_es::event_sink::EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use NonceCommand::*;
        use NonceEvent::*;
        // TODO: add proper errors within NonceError
        // use NonceError::*

        info!("Handling command: {:?}", command);

        let events: Vec<Self::Event> = match command {
            GenerateNonce { c_nonce } => Ok(vec![NonceGenerated {
                c_nonce,
                is_redeemed: false,
            }]),
            RedeemNonce { c_nonce } => Ok(vec![NonceRedeemed {
                c_nonce,
                is_redeemed: true,
            }]),
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use NonceEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            NonceGenerated { c_nonce, is_redeemed } => {
                self.c_nonce = c_nonce;
                self.is_redeemed = is_redeemed;
            }
            NonceRedeemed {
                c_nonce: _,
                is_redeemed,
            } => {
                self.is_redeemed = is_redeemed;
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

    type NonceTestFramework = TestFramework<Nonce>;

    #[rstest]
    #[serial_test::serial]
    async fn test_nonce_generation() {
        let issuance_services = IssuanceServices::default().await;
        let nonce_value = "123-nonce-123".to_string();

        NonceTestFramework::with(issuance_services)
            .given_no_previous_events()
            .when(NonceCommand::GenerateNonce {
                c_nonce: nonce_value.clone(),
            })
            .then_expect_events(vec![NonceEvent::NonceGenerated {
                c_nonce: nonce_value,
                is_redeemed: false,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_nonce_redemption() {
        let issuance_services = IssuanceServices::default().await;
        let nonce_value = "123-nonce-123".to_string();

        NonceTestFramework::with(issuance_services)
            .given(vec![NonceEvent::NonceGenerated {
                c_nonce: nonce_value.clone(),
                is_redeemed: false,
            }])
            .when(NonceCommand::RedeemNonce {
                c_nonce: nonce_value.clone(),
            })
            .then_expect_events(vec![NonceEvent::NonceRedeemed {
                c_nonce: nonce_value,
                is_redeemed: true,
            }]);
    }
}
