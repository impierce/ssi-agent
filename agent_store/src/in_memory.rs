use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agent_issuance::{
    credential::{
        aggregate::Credential, command::CredentialCommand, error::CredentialError, queries::CredentialView,
        services::CredentialServices,
    },
    offer::{
        aggregate::Offer,
        command::OfferCommand,
        error::OfferError,
        queries::{AccessTokenView, OfferSubQuery, OfferView, PreAuthorizedCodeView},
        services::OfferServices,
    },
    server_config::{
        aggregate::ServerConfig, command::ServerConfigCommand, error::ServerConfigError, queries::ServerConfigView,
        services::ServerConfigServices,
    },
    state::{AggregateHandler, ApplicationState, CQRS},
};
use async_trait::async_trait;
use cqrs_es::{
    mem_store::MemStore,
    persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, View,
};

#[derive(Default)]
pub struct MemRepository<V: View<A>, A: Aggregate> {
    pub map: Mutex<HashMap<String, serde_json::Value>>,
    _phantom: std::marker::PhantomData<(V, A)>,
}

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

pub struct OfferAggregateHandler {
    pub main_view: Arc<MemRepository<OfferView, Offer>>,
    pub pre_authorized_code_repo: Arc<MemRepository<PreAuthorizedCodeView, Offer>>,
    pub access_token_repo: Arc<MemRepository<AccessTokenView, Offer>>,
    pub cqrs: Arc<CqrsFramework<Offer, MemStore<Offer>>>,
}

#[async_trait]
impl CQRS<Offer, OfferView> for OfferAggregateHandler {
    async fn new() -> AggregateHandler<Offer, OfferView> {
        let main_view = Arc::new(MemRepository::default());

        let pre_authorized_code_repo = Arc::new(MemRepository::<PreAuthorizedCodeView, Offer>::new());
        let access_token_repo = Arc::new(MemRepository::<AccessTokenView, Offer>::new());

        let mut generic_query = GenericQuery::new(main_view.clone());
        generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let pre_authorized_code_query: OfferSubQuery<
            MemRepository<PreAuthorizedCodeView, Offer>,
            PreAuthorizedCodeView,
        > = OfferSubQuery::new(pre_authorized_code_repo.clone(), "pre-authorized_code".to_string());

        let access_token_query: OfferSubQuery<MemRepository<AccessTokenView, Offer>, AccessTokenView> =
            OfferSubQuery::new(access_token_repo.clone(), "access_token".to_string());

        let cqrs: Arc<CqrsFramework<Offer, MemStore<Offer>>> = Arc::new(CqrsFramework::new(
            MemStore::default(),
            vec![
                Box::new(generic_query),
                Box::new(pre_authorized_code_query),
                Box::new(access_token_query),
            ],
            OfferServices,
        ));

        Arc::new(OfferAggregateHandler {
            main_view,
            pre_authorized_code_repo,
            access_token_repo,
            cqrs,
        })
    }

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: OfferCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<OfferError>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<OfferView>, PersistenceError> {
        self.main_view.load(view_id).await
    }

    async fn load_pre_authorized_code(&self, view_id: &str) -> Result<Option<PreAuthorizedCodeView>, PersistenceError> {
        self.pre_authorized_code_repo.load(view_id).await
    }

    async fn load_access_token(&self, view_id: &str) -> Result<Option<AccessTokenView>, PersistenceError> {
        self.access_token_repo.load(view_id).await
    }
}

pub struct CredentialAggregateHandler {
    pub main_view: Arc<MemRepository<CredentialView, Credential>>,
    pub cqrs: Arc<CqrsFramework<Credential, MemStore<Credential>>>,
}

#[async_trait]
impl CQRS<Credential, CredentialView> for CredentialAggregateHandler {
    async fn new() -> AggregateHandler<Credential, CredentialView> {
        let main_view = Arc::new(MemRepository::default());

        let mut generic_query = GenericQuery::new(main_view.clone());
        generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs: Arc<CqrsFramework<Credential, MemStore<Credential>>> = Arc::new(CqrsFramework::new(
            MemStore::default(),
            vec![Box::new(generic_query)],
            CredentialServices,
        ));

        Arc::new(CredentialAggregateHandler { main_view, cqrs })
    }

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: CredentialCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<CredentialError>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<CredentialView>, PersistenceError> {
        self.main_view.load(view_id).await
    }
}

pub struct ServerConfigAggregateHandler {
    pub main_view: Arc<MemRepository<ServerConfigView, ServerConfig>>,
    pub cqrs: Arc<CqrsFramework<ServerConfig, MemStore<ServerConfig>>>,
}

#[async_trait]
impl CQRS<ServerConfig, ServerConfigView> for ServerConfigAggregateHandler {
    async fn new() -> AggregateHandler<ServerConfig, ServerConfigView> {
        let main_view = Arc::new(MemRepository::default());

        let mut generic_query = GenericQuery::new(main_view.clone());
        generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs: Arc<CqrsFramework<ServerConfig, MemStore<ServerConfig>>> = Arc::new(CqrsFramework::new(
            MemStore::default(),
            vec![Box::new(generic_query)],
            ServerConfigServices,
        ));

        Arc::new(ServerConfigAggregateHandler { main_view, cqrs })
    }

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: ServerConfigCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<ServerConfigError>> {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<ServerConfigView>, PersistenceError> {
        self.main_view.load(view_id).await
    }
}

pub async fn application_state() -> agent_issuance::state::ApplicationState {
    ApplicationState {
        offer: OfferAggregateHandler::new().await,
        credential: CredentialAggregateHandler::new().await,
        server_config: ServerConfigAggregateHandler::new().await,
    }
}
