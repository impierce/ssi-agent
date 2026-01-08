use crate::nonce::command::NonceCommand;
use crate::nonce::error::NonceError;
use crate::nonce::event::NonceEvent;
use crate::services::IssuanceServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Nonce {
    pub c_nonce: String,
    pub is_redeemed: bool,
}

#[async_trait]
impl Aggregate for Nonce {
    type Command = NonceCommand;
    type Event = NonceEvent;
    type Error = NonceError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "nonce".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use NonceCommand::*;
        use NonceError::*;
        use NonceEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            NonceCommand::GenerateNonce { c_nonce } => Ok(vec![NonceEvent::NonceGenerated {
                c_nonce: c_nonce,
                is_redeemed: false,
            }]),
            NonceCommand::RedeemNonce { c_nonce } => Ok(vec![NonceEvent::NonceRedeemed {
                c_nonce: c_nonce,
                is_redeemed: true,
            }]),
        }
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
