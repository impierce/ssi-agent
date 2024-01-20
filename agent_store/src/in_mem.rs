use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::server_config::error::ServerConfigError;
use async_trait::async_trait;
use cqrs_es::AggregateError;
use std::collections::HashMap;
use std::sync::Arc;
use std::{marker::PhantomData, sync::Mutex};

use agent_issuance::{
    server_config::{
        aggregate::ServerConfig,
        queries::{ServerConfigView, SimpleLoggingQuery},
        services::ServerConfigServices,
    },
    state::CQRS,
};
use cqrs_es::{
    mem_store::MemStore,
    persist::{PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, View,
};

#[derive(Default)]
pub struct MemViewRepository<V, A> {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: PhantomData<(V, A)>,
}

impl<V, A> MemViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl<V, A> ViewRepository<V, A> for MemViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        todo!()
    }
    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        todo!()
    }
    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        todo!()
    }
}

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub server_config: Arc<CqrsFramework<A, MemStore<A>>>,
    pub server_config_query: Arc<MemViewRepository<V, A>>,
}

impl ApplicationState {
    pub async fn new(
        queries: Vec<Box<SimpleLoggingQuery>>,
        services: ServerConfigServices,
    ) -> agent_issuance::state::ApplicationState
// where Self: Sized,
    {
        Arc::new(ApplicationState {
            server_config: Arc::new(CqrsFramework::new(MemStore::default(), vec![], services)),
            server_config_query: Arc::new(MemViewRepository::new()),
        })
    }
}

#[async_trait]
impl CQRS for ApplicationState {
    async fn new() {}
    // async fn execute_with_metadata_server_config() {}
    // async fn execute_with_metadata_credential() {}
    // async fn execute_with_metadata_offer() {}

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: ServerConfigCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<ServerConfigError>> {
        self.server_config
            .execute_with_metadata(aggregate_id, command, metadata)
            .await
    }

    async fn load() {}
}

// #[async_trait]
// impl CQRS for ApplicationState {
//     async fn new(queries: Vec<Box<dyn Query<A>>>, services: A::Services) -> agent_issuance::state::ApplicationState {}

//     async fn execute_with_metadata_server_config(
//         &self,
//         aggregate_id: &str,
//         command: A::Command,
//         metadata: HashMap<String, String>,
//     ) -> Result<(), cqrs_es::AggregateError<A::Error>>
//     where
//         A::Command: Send + Sync,
//     {
//         self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
//     }

//     async fn execute_with_metadata_credential(
//         &self,
//         aggregate_id: &str,
//         command: A::Command,
//         metadata: HashMap<String, String>,
//     ) -> Result<(), cqrs_es::AggregateError<A::Error>>
//     where
//         A::Command: Send + Sync,
//     {
//         self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
//     }

//     async fn execute_with_metadata_offer(
//         &self,
//         aggregate_id: &str,
//         command: A::Command,
//         metadata: HashMap<String, String>,
//     ) -> Result<(), cqrs_es::AggregateError<A::Error>>
//     where
//         A::Command: Send + Sync,
//     {
//         self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
//     }

//     async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
//         self.issuance_data_query.load(view_id).await
//     }
// }
