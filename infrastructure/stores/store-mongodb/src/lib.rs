use mongo_es::{default_mongo_client, Client, MongoEventRepository, MongoViewRepository};
use shared_kernel::command_handler::{CommandHandler, CommandHandlerFactory};
use shared_kernel::cqrs_es::View;
use shared_kernel::cqrs_es::{persist::PersistedEventStore, Aggregate, CqrsFramework, Query};
use shared_kernel::view_repository::{BoxedViewRepository, ViewRepositoryFactory};
use std::future::Future;
use std::sync::Arc;

pub struct MongoDBStore {
    client: Client,
}

impl MongoDBStore {
    pub async fn new(connection_string: &str) -> Self {
        let client = default_mongo_client(connection_string).await;
        Self { client }
    }
    // TODO: Run [Client::shutdown] during graceful shutdown to close all open connections.
}

impl ViewRepositoryFactory for MongoDBStore {
    fn create_view_repository<V, A>(&self, name: &str) -> BoxedViewRepository<V, A>
    where
        V: View<A> + Clone + 'static,
        A: Aggregate + 'static,
    {
        BoxedViewRepository::new(Box::new(MongoViewRepository::new(name, self.client.clone())))
    }
}

impl CommandHandlerFactory for MongoDBStore {
    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = CommandHandler<A>> + Send
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send,
    {
        let client = self.client.clone();

        async move {
            let repo = MongoEventRepository::new(client)
                .await
                // Return Result
                .expect("Failed to create MongoEventRepository");
            let store = PersistedEventStore::new_event_store(repo);

            Arc::new(CqrsFramework::new(store, queries, services)) as CommandHandler<A>
        }
    }
}
