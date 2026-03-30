use std::sync::Arc;

use crate::services::VerificationServices;

use super::{
    command::DataAccessConsentTokenCommand, error::DataAccessConsentTokenError, event::DataAccessConsentTokenEvent,
};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DataAccessConsentToken {
    pub id: String,
    pub token: String,
}

#[async_trait]
impl Aggregate for DataAccessConsentToken {
    type Command = DataAccessConsentTokenCommand;
    type Event = DataAccessConsentTokenEvent;
    type Error = DataAccessConsentTokenError;
    type Services = Arc<VerificationServices>;

    fn aggregate_type() -> String {
        "data_access_consent_token".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use DataAccessConsentTokenCommand::*;
        use DataAccessConsentTokenEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            StoreDataAccessConsentToken { id, token } => Ok(vec![DataAccessConsentTokenStored { id, token }]),
            ResolveDataAccessConsentToken { id, called_endpoint } => {
                Ok(vec![DataAccessConsentTokenResolved { id, called_endpoint }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use DataAccessConsentTokenEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            DataAccessConsentTokenStored { id, token } => {
                self.id = id;
                self.token = token;
            }
            DataAccessConsentTokenResolved { .. } => {
                // This event is only for the purpose of storing the event, no new or updated data needs to be stored.
            }
        }
    }
}
