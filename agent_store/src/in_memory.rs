use agent_issuance::credential::aggregate::Credential;
use agent_issuance::credential::command::CredentialCommand;
use agent_issuance::credential::error::CredentialError;
use agent_issuance::credential::services::CredentialServices;
use agent_issuance::offer::aggregate::Offer;
use agent_issuance::offer::command::OfferCommand;
use agent_issuance::offer::error::OfferError;
use agent_issuance::offer::services::OfferServices;
use agent_issuance::server_config::aggregate::ServerConfig;
use agent_issuance::server_config::command::ServerConfigCommand;
use agent_issuance::server_config::error::ServerConfigError;
use agent_issuance::server_config::services::ServerConfigServices;
use agent_issuance::state::CQRS;
use async_trait::async_trait;
use cqrs_es::mem_store::MemStore;
use cqrs_es::persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository};
use cqrs_es::CqrsFramework;
use cqrs_es::{Aggregate, Query, View};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemRepository<V: View<A>, A: Aggregate> {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: std::marker::PhantomData<(V, A)>,
}

#[derive(Default)]
pub struct MemRepositoryNoView {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
}

// #[async_trait]
// impl<V, A> ViewRepository<V, A> for MemRepositoryNoView
// where
//     V: View<A>,
//     A: Aggregate,
// {
//     async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
//         Ok(self
//             .map
//             .lock()
//             .unwrap()
//             .get(view_id)
//             .map(|view| serde_json::from_value(view.clone()).unwrap()))
//     }

//     async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
//         Ok(self.map.lock().unwrap().get(view_id).map(|view| {
//             let view = serde_json::from_value(view.clone()).unwrap();
//             let view_context = ViewContext::new(view_id.to_string(), 0);
//             (view, view_context)
//         }))
//     }

//     async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
//         let payload = serde_json::to_value(&view).unwrap();
//         self.map.lock().unwrap().insert(context.view_instance_id, payload);
//         Ok(())
//     }
// }

impl<V: View<A>, A: Aggregate> MemRepository<V, A> {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl<V, A> ViewRepository<V, A> for MemRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .unwrap()
            .get(view_id)
            .map(|view| serde_json::from_value(view.clone()).unwrap()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(V, ViewContext)>, PersistenceError> {
        Ok(self.map.lock().unwrap().get(view_id).map(|view| {
            let view = serde_json::from_value(view.clone()).unwrap();
            let view_context = ViewContext::new(view_id.to_string(), 0);
            (view, view_context)
        }))
    }

    async fn update_view(&self, view: V, context: ViewContext) -> Result<(), PersistenceError> {
        let payload = serde_json::to_value(&view).unwrap();
        self.map.lock().unwrap().insert(context.view_instance_id, payload);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ApplicationState {
    pub server_config: Arc<CqrsFramework<ServerConfig, MemStore<ServerConfig>>>,
    // pub server_config_query: Arc<MemRepository<dyn View<ServerConfig>, ServerConfig>>,
    pub server_config_query: Arc<MemRepositoryNoView>,

    pub credential: Arc<CqrsFramework<Credential, MemStore<Credential>>>,
    // pub credential_query: Arc<MemRepository<dyn View<Credential>, Credential>>,
    pub credential_query: Arc<MemRepositoryNoView>,

    pub offer: Arc<CqrsFramework<Offer, MemStore<Offer>>>,
    // pub offer_query: Arc<MemRepository<dyn View<Offer>, Offer>>,
    pub offer_query: Arc<MemRepositoryNoView>,
}

// impl<A, V> ApplicationState<A, V>
// where
//     A: Aggregate + 'static,
//     V: View<A> + 'static,
// {
// }

#[async_trait]
impl CQRS for ApplicationState {
    async fn new(
        server_config_queries: Vec<Box<dyn Query<ServerConfig>>>,
        server_config_services: ServerConfigServices,
    ) -> agent_issuance::state::ApplicationState
    where
        Self: Sized,
    {
        let credential_view_repo = Arc::new(MemRepositoryNoView::default());
        // let mut issuance_data_query = GenericQuery::new(credential_view_repo.clone());
        // issuance_data_query.use_error_handler(Box::new(|e| println!("{}", e)));

        // let mut queries = server_config_queries;
        // queries.push(Box::new(issuance_data_query));

        Arc::new(ApplicationState {
            server_config: Arc::new(CqrsFramework::new(
                MemStore::default(),
                server_config_queries,
                server_config_services,
            )),
            server_config_query: credential_view_repo,
            credential: Arc::new(CqrsFramework::new(MemStore::default(), vec![], CredentialServices {})),
            credential_query: Arc::new(MemRepositoryNoView::default()),
            offer: Arc::new(CqrsFramework::new(MemStore::default(), vec![], OfferServices {})),
            offer_query: Arc::new(MemRepositoryNoView::default()),
            // cqrs: Arc::new(CqrsFramework::new(MemStore::default(), queries, services)),
            // issuance_data_query: credential_view_repo,
        }) as agent_issuance::state::ApplicationState
    }

    async fn execute_with_metadata_server_config(
        &self,
        aggregate_id: &str,
        command: ServerConfigCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<ServerConfigError>>
// where
    //     A::Command: Send + Sync,
    {
        self.server_config
            .execute_with_metadata(aggregate_id, command, metadata)
            .await
    }

    async fn execute_with_metadata_credential(
        &self,
        aggregate_id: &str,
        command: CredentialCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<CredentialError>> {
        self.credential
            .execute_with_metadata(aggregate_id, command, metadata)
            .await
    }

    async fn execute_with_metadata_offer(
        &self,
        aggregate_id: &str,
        command: OfferCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<OfferError>> {
        self.offer.execute_with_metadata(aggregate_id, command, metadata).await
    }

    // async fn load(&self, view_id: &str) -> Result<Option<serde_json::Value>, PersistenceError> {
    //     self.server_config_query.load(view_id).await
    //     // Ok(None)
    // }

    async fn load(&self, view_id: &str) -> Result<Option<serde_json::Value>, PersistenceError> {
        self.server_config_query.load(view_id).await
        // Ok(None)
    }
}
