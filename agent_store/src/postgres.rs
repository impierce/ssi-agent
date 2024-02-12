use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use agent_issuance::{
    credential::{aggregate::Credential, queries::CredentialView, services::CredentialServices},
    offer::{
        aggregate::Offer,
        queries::{AccessTokenView, OfferSubQuery, OfferView, PreAuthorizedCodeView},
        services::OfferServices,
    },
    server_config::{aggregate::ServerConfig, queries::ServerConfigView, services::ServerConfigServices},
    state::{TestAggregateHandler, TestApplicationState, TestCQRS, CQRS},
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
impl TestCQRS<PostgresViewRepository<OfferView, Offer>, Offer, OfferView> for OfferAggregateHandler {
    async fn new() -> TestAggregateHandler<PostgresViewRepository<OfferView, Offer>, Offer, OfferView> {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("offer_query", pool.clone()));
        let mut offer_query = GenericQuery::new(main_view.clone());
        offer_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let pre_authorized_code_repo = Arc::new(PostgresViewRepository::new("pre_authorized_code_query", pool.clone()));
        let access_token_repo = Arc::new(PostgresViewRepository::new("access_token_query", pool.clone()));

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
}

pub struct CredentialAggregateHandler {
    pub main_view: Arc<PostgresViewRepository<CredentialView, Credential>>,
    pub cqrs: Arc<PostgresCqrs<Credential>>,
}

#[async_trait]
impl TestCQRS<PostgresViewRepository<CredentialView, Credential>, Credential, CredentialView>
    for CredentialAggregateHandler
{
    async fn new(
    ) -> TestAggregateHandler<PostgresViewRepository<CredentialView, Credential>, Credential, CredentialView> {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("credential_query", pool.clone()));
        let mut credential_query = GenericQuery::new(main_view.clone());
        credential_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs = Arc::new(postgres_es::postgres_cqrs(
            pool,
            vec![Box::new(credential_query)],
            CredentialServices,
        ));

        Arc::new(CredentialAggregateHandler { main_view, cqrs })
    }
}

pub struct ServerConfigAggregateHandler {
    pub main_view: Arc<PostgresViewRepository<ServerConfigView, ServerConfig>>,
    pub cqrs: Arc<PostgresCqrs<ServerConfig>>,
}

#[async_trait]
impl TestCQRS<PostgresViewRepository<ServerConfigView, ServerConfig>, ServerConfig, ServerConfigView>
    for ServerConfigAggregateHandler
{
    async fn new(
    ) -> TestAggregateHandler<PostgresViewRepository<ServerConfigView, ServerConfig>, ServerConfig, ServerConfigView>
    {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;

        let main_view = Arc::new(PostgresViewRepository::new("server_config_query", pool.clone()));
        let mut server_config_query = GenericQuery::new(main_view.clone());
        server_config_query.use_error_handler(Box::new(|e| println!("{}", e)));

        let cqrs = Arc::new(postgres_es::postgres_cqrs(
            pool,
            vec![Box::new(server_config_query)],
            ServerConfigServices,
        ));

        Arc::new(ServerConfigAggregateHandler { main_view, cqrs })
    }
}

pub async fn application_state() -> agent_issuance::state::TestApplicationState<
    PostgresViewRepository<OfferView, Offer>,
    PostgresViewRepository<CredentialView, Credential>,
    PostgresViewRepository<ServerConfigView, ServerConfig>,
> {
    TestApplicationState {
        offer: OfferAggregateHandler::new().await,
        credential: CredentialAggregateHandler::new().await,
        server_config: ServerConfigAggregateHandler::new().await,
    }
}

pub fn cqrs_framework<A, V>(
    pool: Pool<Postgres>,
    mut queries: Vec<Box<dyn Query<A>>>,
    services: A::Services,
) -> (Arc<PostgresCqrs<A>>, Arc<PostgresViewRepository<V, A>>)
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
{
    // A query that stores the current state of an individual account.
    let issuance_data_repo = Arc::new(PostgresViewRepository::new("issuance_data_query", pool.clone()));
    let mut issuance_data_query = GenericQuery::new(issuance_data_repo.clone());
    issuance_data_query.use_error_handler(Box::new(|e| println!("{}", e)));

    // Create and return an event-sourced `CqrsFramework`.
    queries.push(Box::new(issuance_data_query));
    (
        Arc::new(postgres_es::postgres_cqrs(pool, queries, services)),
        issuance_data_repo,
    )
}
