use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::view_repository::DynViewRepository;
use agent_shared::{application_state::Command, config::config};
use cqrs_es::persist::PersistedEventStore;
use cqrs_es::{
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, Query, View,
};
use eventsourcingdb_es::{default_client, EventSourcingDbEventRepository};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Default)]
struct MemRepository<V: View<A>, A: Aggregate> {
    map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: std::marker::PhantomData<(V, A)>,
}

impl<V, A> ViewRepository<V, A> for MemRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .await
            .get(view_id)
            .map(|view| serde_json::from_value(view.clone()).unwrap()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self.map.lock().await.get(view_id).map(|view| {
            let view = serde_json::from_value(view.clone()).unwrap();
            let view_context = ViewContext::new(view_id.to_string(), 0);
            (view, view_context)
        }))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        let payload = serde_json::to_value(&view).unwrap();
        self.map.lock().await.insert(context.view_instance_id, payload);
        Ok(())
    }
}

impl<A> AggregateHandler<A, PersistedEventStore<EventSourcingDbEventRepository, A>>
where
    A: Aggregate,
{
    async fn new(repository: EventSourcingDbEventRepository, services: A::Services) -> Self {
        let store = PersistedEventStore::new_event_store(repository);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
        }
    }
}

pub struct EventSourcingDb {
    pub connection_string: String,
    pub api_token: String,
}

impl EventSourcingDb {
    pub async fn new() -> Self {
        let event_store = &config().event_store;

        let connection_string = event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let api_token = event_store
            .api_token
            .clone()
            .expect("Missing config parameter `event_store.api_token` or `UNICORE__EVENT_STORE__API_TOKEN`");

        Self {
            connection_string,
            api_token,
        }
    }
}

impl CqrsComponentBuilder for EventSourcingDb {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn DynViewRepository<V, A>>,
        Arc<dyn DynViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::TYPE);

        // EventSourcingDB currently stores events remotely and keeps views in-process.
        let aggregate: Arc<MemRepository<V, A>> = Arc::new(MemRepository::default());
        let all_aggregates: Arc<MemRepository<AV, A>> = Arc::new(MemRepository::default());

        let base_url = self
            .connection_string
            .parse()
            .expect("Invalid EventSourcingDB URL in `event_store.connection_string`");

        let client = default_client(base_url, self.api_token.clone()).await;

        let repository = EventSourcingDbEventRepository::new(client)
            .await
            .expect("Failed to create EventSourcingDbEventRepository");

        (
            Arc::new(AggregateHandler::new(repository, services).await.with_parameters(
                aggregate.clone(),
                all_aggregates.clone(),
                event_publishers,
                &all_aggregates_name,
            )),
            aggregate,
            all_aggregates,
        )
    }
}
