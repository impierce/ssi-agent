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
use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::{
    mem_store::MemStore,
    persist::{GenericQuery, PersistenceError, ViewContext, ViewRepository},
    Aggregate, CqrsFramework, Query, View,
};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};

pub struct OfferAggregateHandler {
    pub main_view: Arc<PostgresViewRepository<OfferView, Offer>>,
    pub pre_authorized_code_repo: Arc<PostgresViewRepository<PreAuthorizedCodeView, Offer>>,
    pub access_token_repo: Arc<PostgresViewRepository<AccessTokenView, Offer>>,
    pub cqrs: Arc<PostgresCqrs<Offer>>,
}

#[async_trait]
impl CQRS<Offer, OfferView> for OfferAggregateHandler {
    async fn new() -> AggregateHandler<Offer, OfferView> {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("offer", pool.clone()));
        let mut offer_query = GenericQuery::new(main_view.clone());
        offer_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let pre_authorized_code_repo = Arc::new(PostgresViewRepository::new("pre_authorized_code", pool.clone()));
        let access_token_repo = Arc::new(PostgresViewRepository::new("access_token", pool.clone()));

        let mut generic_query = GenericQuery::new(main_view.clone());
        generic_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let pre_authorized_code_query: OfferSubQuery<
            PostgresViewRepository<PreAuthorizedCodeView, Offer>,
            PreAuthorizedCodeView,
        > = OfferSubQuery::new(pre_authorized_code_repo.clone(), "pre-authorized_code".to_string());

        let access_token_query: OfferSubQuery<PostgresViewRepository<AccessTokenView, Offer>, AccessTokenView> =
            OfferSubQuery::new(access_token_repo.clone(), "access_token".to_string());

        let cqrs = Arc::new(postgres_es::postgres_cqrs(
            pool,
            vec![
                Box::new(offer_query),
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
    pub main_view: Arc<PostgresViewRepository<CredentialView, Credential>>,
    pub cqrs: Arc<PostgresCqrs<Credential>>,
}

#[async_trait]
impl CQRS<Credential, CredentialView> for CredentialAggregateHandler {
    async fn new() -> AggregateHandler<Credential, CredentialView> {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("credential", pool.clone()));
        let mut credential_query = GenericQuery::new(main_view.clone());
        credential_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs = Arc::new(postgres_es::postgres_cqrs(
            pool,
            vec![Box::new(credential_query)],
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
    pub main_view: Arc<PostgresViewRepository<ServerConfigView, ServerConfig>>,
    pub cqrs: Arc<PostgresCqrs<ServerConfig>>,
}

#[async_trait]
impl CQRS<ServerConfig, ServerConfigView> for ServerConfigAggregateHandler {
    async fn new() -> AggregateHandler<ServerConfig, ServerConfigView> {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("server_config", pool.clone()));
        let mut server_config_query = GenericQuery::new(main_view.clone());
        server_config_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs = Arc::new(postgres_es::postgres_cqrs(
            pool,
            vec![Box::new(server_config_query)],
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
