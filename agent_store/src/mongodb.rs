use crate::{AggregateHandler, CqrsComponentBuilder};
use shared_kernel::view_repository::DynViewRepository;
use agent_shared::{application_state::Command, config::config};
use cqrs_es::{persist::PersistedEventStore, CqrsFramework};
use cqrs_es::{Aggregate, Query, View};
use mongo_es::{default_mongo_client, Client, MongoEventRepository, MongoViewRepository};
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<MongoEventRepository, A>>
where
    A: Aggregate,
{
    async fn new(client: Client, services: A::Services) -> Self {
        let repo = MongoEventRepository::new(client)
            .await
            .expect("Failed to create MongoEventRepository");
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
        }
    }
}

pub struct MongoDB {
    pub client: Client,
}

impl MongoDB {
    pub async fn new() -> Self {
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let client = default_mongo_client(&connection_string).await;
        Self { client }
    }
    // TODO: Run [Client::shutdown] during graceful shutdown to close all open connections.
}

impl CqrsComponentBuilder for MongoDB {
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

        // Initialize the MongoDB repositories.
        let aggregate: Arc<MongoViewRepository<V, A>> =
            Arc::new(MongoViewRepository::new(A::TYPE, self.client.clone()));
        let all_aggregates: Arc<MongoViewRepository<AV, A>> =
            Arc::new(MongoViewRepository::new(&all_aggregates_name, self.client.clone()));

        (
            Arc::new(
                AggregateHandler::new(self.client.clone(), services)
                    .await
                    .with_parameters(
                        aggregate.clone(),
                        all_aggregates.clone(),
                        event_publishers,
                        &all_aggregates_name,
                    ),
            ),
            aggregate,
            all_aggregates,
        )
    }
}
