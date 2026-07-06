use crate::command_handler::{CommandHandler, CommandHandlerFactory, EventUpcaster};
use crate::view_repository::{BoxedViewRepository, ViewRepositoryFactory};
use cqrs_es::persist::{
    PersistedEventRepository, PersistedEventStore, PersistenceError, ReplayStream, SerializedEvent,
    SerializedSnapshot, ViewContext, ViewRepository as CoreViewRepository,
};
use cqrs_es::CqrsFramework;
use cqrs_es::{Aggregate, Query, View};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, RwLock};

/// An in-memory store backend that implements [`ViewRepositoryFactory`] and [`CommandHandlerFactory`].
///
/// Useful for unit and integration tests where persistence is not needed.
pub struct InMemoryStore;

impl ViewRepositoryFactory for InMemoryStore {
    fn create_view_repository<V, A>(&self, _name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static,
    {
        BoxedViewRepository::new(Box::new(MemViewRepository::default()))
    }
}

impl CommandHandlerFactory for InMemoryStore {
    type Error = std::convert::Infallible;

    async fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
        upcasters: Vec<Box<dyn EventUpcaster>>,
    ) -> Result<CommandHandler<A>, Self::Error>
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send,
    {
        // Unlike `cqrs_es::mem_store::MemStore`, `InMemoryEventRepository` actually
        // (de)serializes events through `SerializedEvent`, so registered `EventUpcaster`s
        // run exactly as they would against a real backing store.
        let store = PersistedEventStore::new_event_store(InMemoryEventRepository::default())
            .with_upcasters(upcasters);

        Ok(Arc::new(CqrsFramework::new(store, queries, services)) as CommandHandler<A>)
    }
}

/// A minimal in-memory [`PersistedEventRepository`], storing [`SerializedEvent`]s per
/// aggregate instance behind a `HashMap`.
///
/// Snapshots are not supported (the codebase doesn't use them): [`Self::get_snapshot`]
/// always returns `None` and `persist` ignores any snapshot update.
#[derive(Clone, Default)]
pub struct InMemoryEventRepository {
    events: Arc<RwLock<HashMap<String, Vec<SerializedEvent>>>>,
}

impl PersistedEventRepository for InMemoryEventRepository {
    async fn get_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
    ) -> Result<Vec<SerializedEvent>, PersistenceError> {
        Ok(self
            .events
            .read()
            .await
            .get(aggregate_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
        last_sequence: usize,
    ) -> Result<Vec<SerializedEvent>, PersistenceError> {
        Ok(self
            .events
            .read()
            .await
            .get(aggregate_id)
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event.sequence > last_sequence)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn get_snapshot<A: Aggregate>(
        &self,
        _aggregate_id: &str,
    ) -> Result<Option<SerializedSnapshot>, PersistenceError> {
        Ok(None)
    }

    async fn persist<A: Aggregate>(
        &self,
        events: &[SerializedEvent],
        _snapshot_update: Option<(String, Value, usize)>,
    ) -> Result<(), PersistenceError> {
        let mut store = self.events.write().await;
        for event in events {
            store.entry(event.aggregate_id.clone()).or_default().push(event.clone());
        }
        Ok(())
    }

    async fn stream_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
    ) -> Result<ReplayStream, PersistenceError> {
        let events = self.get_events::<A>(aggregate_id).await?;
        let (mut feed, stream) = ReplayStream::new(events.len().max(1));
        for event in events {
            feed.push(Ok(event)).await?;
        }
        Ok(stream)
    }

    async fn stream_all_events<A: Aggregate>(&self) -> Result<ReplayStream, PersistenceError> {
        let events: Vec<SerializedEvent> = self.events.read().await.values().flatten().cloned().collect();
        let (mut feed, stream) = ReplayStream::new(events.len().max(1));
        for event in events {
            feed.push(Ok(event)).await?;
        }
        Ok(stream)
    }
}

/// A simple in-memory [`ViewRepository`](CoreViewRepository) backed by a `HashMap`.
///
/// Clones views on read. Suitable for testing only.
#[derive(Default)]
pub struct MemViewRepository<V: View<A> + Clone, A: Aggregate> {
    map: Mutex<HashMap<String, V>>,
    _phantom: std::marker::PhantomData<A>,
}

impl<V, A> CoreViewRepository<V, A> for MemViewRepository<V, A>
where
    V: View<A> + Clone,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self.map.lock().await.get(view_id).cloned())
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .await
            .get(view_id)
            .map(|view| (view.clone(), ViewContext::new(view_id.to_string(), 0))))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        self.map.lock().await.insert(context.view_instance_id, view);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent, EventStore};
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    // ── Test aggregate: an `Account` whose `AccountOpened` event gained a
    //    `currency` field between version "1" and version "2". ──────────

    #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
    struct Account {
        balance: i64,
        currency: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct AccountOpened {
        balance: i64,
        currency: String,
    }

    impl DomainEvent for AccountOpened {
        fn event_type(&self) -> String {
            "AccountOpened".to_string()
        }

        fn event_version(&self) -> String {
            "2".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct AccountError(String);

    impl Aggregate for Account {
        const TYPE: &'static str = "Account";
        type Command = ();
        type Event = AccountOpened;
        type Error = AccountError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _services: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, event: Self::Event) {
            self.balance = event.balance;
            self.currency = event.currency;
        }
    }

    /// Upcasts version "1" `AccountOpened` events (which had no `currency` field) to
    /// version "2" by defaulting the currency to `"USD"`.
    struct AddDefaultCurrencyUpcaster;

    impl EventUpcaster for AddDefaultCurrencyUpcaster {
        fn can_upcast(&self, event_type: &str, event_version: &str) -> bool {
            event_type == "AccountOpened" && event_version == "1"
        }

        fn upcast(&self, event: SerializedEvent) -> SerializedEvent {
            let mut payload = event.payload;
            if let Value::Object(ref mut map) = payload {
                map.insert("currency".to_string(), json!("USD"));
            }
            SerializedEvent {
                event_version: "2".to_string(),
                payload,
                ..event
            }
        }
    }

    #[tokio::test]
    async fn upcaster_rewrites_old_events_loaded_through_persisted_store() {
        let repo = InMemoryEventRepository::default();

        // Simulate an event persisted under the old (version "1") shape: no `currency` field.
        let old_event = SerializedEvent::new(
            "acc-1".to_string(),
            1,
            Account::TYPE.to_string(),
            "AccountOpened".to_string(),
            "1".to_string(),
            json!({ "balance": 100 }),
            json!({}),
        );
        repo.persist::<Account>(&[old_event], None).await.unwrap();

        let store = PersistedEventStore::<InMemoryEventRepository, Account>::new_event_store(repo)
            .with_upcasters(vec![Box::new(AddDefaultCurrencyUpcaster)]);

        // Reading the raw events applies the upcaster and yields the current event shape.
        let events = store.load_events("acc-1").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].payload,
            AccountOpened {
                balance: 100,
                currency: "USD".to_string(),
            }
        );

        // Loading the aggregate applies the upcasted event too.
        let context = store.load_aggregate("acc-1").await.unwrap();
        assert_eq!(
            context.aggregate,
            Account {
                balance: 100,
                currency: "USD".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn create_handler_returns_a_working_handler() {
        use crate::command_handler::dispatch;

        let handler = InMemoryStore
            .create_handler::<Account>((), vec![], vec![])
            .await
            .unwrap();

        // No upcasters registered and no prior events: dispatching should simply succeed.
        dispatch::<Account>(&handler, "acc-2", ()).await.unwrap();
    }
}
