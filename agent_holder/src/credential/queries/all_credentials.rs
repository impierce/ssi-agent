use crate::credential::queries::{Credential, CustomQuery, ViewRepository};
use async_trait::async_trait;
use cqrs_es::{
    persist::{PersistenceError, ViewContext},
    EventEnvelope, Query, View,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{collections::HashMap, marker::PhantomData};

use super::CredentialView;

const VIEW_ID: &str = "all_credentials";

/// A custom query trait for the Credential aggregate. This query is used to update the `AllCredentialsView`.
pub struct AllCredentialsQuery<R, V>
where
    R: ViewRepository<V, Credential>,
    V: View<Credential>,
{
    view_repository: Arc<R>,
    _phantom: PhantomData<V>,
}

impl<R, V> AllCredentialsQuery<R, V>
where
    R: ViewRepository<V, Credential>,
    V: View<Credential>,
{
    pub fn new(view_repository: Arc<R>) -> Self {
        AllCredentialsQuery {
            view_repository,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<R, V> Query<Credential> for AllCredentialsQuery<R, V>
where
    R: ViewRepository<V, Credential>,
    V: View<Credential>,
{
    // The `dispatch` method is called by the `CqrsFramework` when an event is published. By default `cqrs` will use the
    // `aggregate_id` as the `view_id` when calling the `dispatch` method. We override this behavior by using the
    // `VIEW_ID` constant as the `view_id`.
    async fn dispatch(&self, _view_id: &str, events: &[EventEnvelope<Credential>]) {
        self.apply_events(VIEW_ID, events).await.ok();
    }
}

#[async_trait]
impl<R, V> CustomQuery<R, V> for AllCredentialsQuery<R, V>
where
    R: ViewRepository<V, Credential>,
    V: View<Credential>,
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

    async fn apply_events(&self, view_id: &str, events: &[EventEnvelope<Credential>]) -> Result<(), PersistenceError> {
        for event in events {
            let (mut view, view_context) = self.load_mut(view_id.to_string()).await?;

            view.update(event);
            self.view_repository.update_view(view, view_context).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct AllCredentialsView {
    pub credentials: HashMap<String, CredentialView>,
}

impl View<Credential> for AllCredentialsView {
    fn update(&mut self, event: &EventEnvelope<Credential>) {
        self.credentials
            // Get the entry for the aggregate_id
            .entry(event.aggregate_id.clone())
            // or insert a new one if it doesn't exist
            .or_default()
            // update the view with the event
            .update(event);
    }
}
