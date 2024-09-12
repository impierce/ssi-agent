use crate::offer::queries::{Offer, OfferEvent, ViewRepository};
use agent_shared::custom_queries::CustomQuery;
use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext},
    EventEnvelope, Query, View,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;

/// A custom query trait for the Offer aggregate. This query is used to update the `PreAuthorizedCodeView`.
pub struct PreAuthorizedCodeQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    view_repository: Arc<R>,
    _phantom: PhantomData<V>,
}

impl<R, V> PreAuthorizedCodeQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    pub fn new(view_repository: Arc<R>) -> Self {
        PreAuthorizedCodeQuery {
            view_repository,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<R, V> Query<Offer> for PreAuthorizedCodeQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    async fn dispatch(&self, view_id: &str, events: &[EventEnvelope<Offer>]) {
        self.apply_events(view_id, events).await.ok();
    }
}

#[async_trait]
impl<R, V> CustomQuery<R, V, Offer> for PreAuthorizedCodeQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    async fn load_mut(&self, view_id: String) -> Result<(V, ViewContext), PersistenceError> {
        match self.view_repository.load_with_context(&view_id).await? {
            None => {
                let view_context = ViewContext::new(view_id, 0);
                Ok((Default::default(), view_context))
            }
            Some((view, context)) => Ok((view, context)),
        }
    }

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<Offer>]) -> Result<(), PersistenceError> {
        for event in events {
            let (mut view, mut view_context) = self.load_mut(view_id.to_string()).await?;
            if let OfferEvent::CredentialOfferCreated {
                pre_authorized_code, ..
            } = &event.payload
            {
                view_context.view_instance_id.clone_from(pre_authorized_code);
                view.update(event);
                self.view_repository.update_view(view, view_context).await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PreAuthorizedCodeView {
    pub offer_id: String,
}

impl View<Offer> for PreAuthorizedCodeView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        use crate::offer::event::OfferEvent::*;

        if let CredentialOfferCreated { .. } = event.payload {
            self.offer_id.clone_from(&event.aggregate_id)
        }
    }
}
