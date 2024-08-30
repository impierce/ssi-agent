use crate::offer::queries::{CustomQuery, Offer, ViewRepository};
use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext},
    EventEnvelope, Query, View,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{collections::HashMap, marker::PhantomData};

use super::OfferView;

const VIEW_ID: &str = "all_offers";

/// A custom query trait for the Offer aggregate. This query is used to update the `AllOffersView`.
pub struct AllOffersQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    view_repository: Arc<R>,
    _phantom: PhantomData<V>,
}

impl<R, V> AllOffersQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    pub fn new(view_repository: Arc<R>) -> Self {
        AllOffersQuery {
            view_repository,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<R, V> Query<Offer> for AllOffersQuery<R, V>
where
    R: ViewRepository<V, Offer>,
    V: View<Offer>,
{
    // The `dispatch` method is called by the `CqrsFramework` when an event is published. By default `cqrs` will use the
    // `aggregate_id` as the `view_id` when calling the `dispatch` method. We override this behavior by using the
    // `VIEW_ID` constant as the `view_id`.
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<Offer>]) {
        self.apply_events(VIEW_ID, events).await.ok();
    }
}

#[async_trait]
impl<R, V> CustomQuery<R, V> for AllOffersQuery<R, V>
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
            let (mut view, view_context) = self.load_mut(view_id.to_string()).await?;

            view.update(event);
            self.view_repository.update_view(view, view_context).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllOffersView {
    pub offers: HashMap<String, OfferView>,
}

impl View<Offer> for AllOffersView {
    fn update(&mut self, event: &EventEnvelope<Offer>) {
        self.offers
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
