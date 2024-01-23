use agent_issuance::credential::services::CredentialServices;
use agent_issuance::offer::services::OfferServices;
use agent_issuance::server_config::services::ServerConfigServices;
use agent_issuance::state::{ApplicationState, Domain, CQRS};
use async_trait::async_trait;
use cqrs_es::mem_store::MemStore;
use cqrs_es::persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository};
use cqrs_es::CqrsFramework;
use cqrs_es::{Aggregate, Query, View};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemRepository<D: Domain> {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: std::marker::PhantomData<(D::View, D::Aggregate)>,
}

impl<D: Domain> MemRepository<D> {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D> ViewRepository<D::View, D::Aggregate> for MemRepository<D>
where
    D: Domain,
{
    async fn load(&self, view_id: &str) -> Result<Option<D::View>, PersistenceError> {
        Ok(self
            .map
            .lock()
            .unwrap()
            .get(view_id)
            .map(|view| serde_json::from_value(view.clone()).unwrap()))
    }

    async fn load_with_context(&self, view_id: &str) -> Result<Option<(D::View, ViewContext)>, PersistenceError> {
        Ok(self.map.lock().unwrap().get(view_id).map(|view| {
            let view = serde_json::from_value(view.clone()).unwrap();
            let view_context = ViewContext::new(view_id.to_string(), 0);
            (view, view_context)
        }))
    }

    async fn update_view(&self, view: D::View, context: ViewContext) -> Result<(), PersistenceError> {
        let payload = serde_json::to_value(&view).unwrap();
        self.map.lock().unwrap().insert(context.view_instance_id, payload);
        Ok(())
    }
}

pub async fn application_state() -> ApplicationState {
    ApplicationState {
        server_config: AggregateHandler::new(vec![], ServerConfigServices).await,
        credential: AggregateHandler::new(vec![], CredentialServices).await,
        offer: AggregateHandler::new(vec![], OfferServices).await,
    }
}

#[derive(Clone)]
pub struct AggregateHandler<D: Domain> {
    pub cqrs: Arc<CqrsFramework<D::Aggregate, MemStore<D::Aggregate>>>,
    pub issuance_data_query: Arc<MemRepository<D>>,
}

#[async_trait]
impl<D: Domain + 'static> CQRS<D> for AggregateHandler<D> {
    async fn new(
        queries: Vec<Box<dyn Query<D::Aggregate>>>,
        services: <D::Aggregate as Aggregate>::Services,
    ) -> agent_issuance::state::AggregateHandler<D>
    where
        Self: Sized,
    {
        let credential_view_repo = Arc::new(MemRepository::<D>::new());
        let mut issuance_data_query = GenericQuery::new(credential_view_repo.clone());
        issuance_data_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let mut queries = queries;
        queries.push(Box::new(issuance_data_query));

        Arc::new(AggregateHandler {
            cqrs: Arc::new(CqrsFramework::new(MemStore::default(), queries, services)),
            issuance_data_query: credential_view_repo,
        }) as agent_issuance::state::AggregateHandler<D>
    }

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: <D::Aggregate as Aggregate>::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<<D::Aggregate as Aggregate>::Error>>
    where
        <D::Aggregate as Aggregate>::Command: Send + Sync,
    {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<D::View>, PersistenceError> {
        // self.issuance_data_query.load(view_id).await
        Ok(None)
    }
}
