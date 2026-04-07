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

// TODO: re-expose `mongodb::error::Result` through `mongo_es` and use it as the error type here instead of defining a
// new one.
#[derive(Debug, thiserror::Error)]
#[error("MongoDB aggregate error: {0}")]
pub struct MongoDBAggregateError(String);

impl CommandHandlerFactory for MongoDBStore {
    type Error = MongoDBAggregateError;

    fn create_handler<A>(
        &self,
        services: A::Services,
        queries: Vec<Box<dyn Query<A>>>,
    ) -> impl Future<Output = Result<CommandHandler<A>, Self::Error>> + Send
    where
        A: Aggregate + 'static,
        <A as Aggregate>::Command: Send,
    {
        let client = self.client.clone();

        async move {
            let repo = MongoEventRepository::new(client)
                .await
                .map_err(|e| MongoDBAggregateError(e.to_string()))?;
            let store = PersistedEventStore::new_event_store(repo);

            Ok(Arc::new(CqrsFramework::new(store, queries, services)) as CommandHandler<A>)
        }
    }
}
