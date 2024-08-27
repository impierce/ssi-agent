pub mod all_credentials;

use super::event::CredentialEvent;
use crate::credential::aggregate::Credential;
use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    EventEnvelope, Query, View,
};
use serde::{Deserialize, Serialize};

/// A custom query trait for the Credential aggregate. This trait is used to define custom queries for the Credential aggregate
/// that do not make use of `GenericQuery`.
#[async_trait]
pub trait CustomQuery<R, V>: Query<Credential>
where
    R: ViewRepository<V, Credential>,
    V: View<Credential>,
{
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError>;

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<Credential>]) -> Result<(), PersistenceError>;
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CredentialView {
    pub credential_id: Option<String>,
    pub offer_id: Option<String>,
    pub credential: Option<serde_json::Value>,
}

impl View<Credential> for CredentialView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        use CredentialEvent::*;

        match &event.payload {
            CredentialAdded {
                credential_id,
                offer_id,
                credential,
            } => {
                self.credential_id.replace(credential_id.clone());
                self.offer_id.replace(offer_id.clone());
                self.credential.replace(credential.clone());
            }
        }
    }
}
