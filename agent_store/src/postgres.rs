use agent_issuance::credential::services::CredentialServices;
use agent_issuance::offer::services::OfferServices;
use agent_issuance::server_config::services::ServerConfigServices;
use agent_issuance::state::{ApplicationState, Domain, CQRS};
use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::persist::{GenericQuery, PersistenceError, ViewRepository};
use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn application_state() -> ApplicationState {
    ApplicationState {
        server_config: AggregateHandler::new(vec![], ServerConfigServices).await,
        credential: AggregateHandler::new(vec![], CredentialServices).await,
        offer: AggregateHandler::new(vec![], OfferServices).await,
    }
}

#[derive(Clone)]
pub struct AggregateHandler<D: Domain> {
    pub cqrs: Arc<PostgresCqrs<D::Aggregate>>,
    pub issuance_data_query: Arc<PostgresViewRepository<D::View, D::Aggregate>>,
}

#[async_trait]
impl<D> CQRS<D> for AggregateHandler<D>
where
    D: Domain + 'static,
{
    async fn new(
        queries: Vec<Box<dyn Query<D::Aggregate>>>,
        services: <D::Aggregate as Aggregate>::Services,
    ) -> agent_issuance::state::AggregateHandler<D>
    where
        Self: Sized,
    {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;
        let (cqrs, issuance_data_query) = cqrs_framework(pool, queries, services);
        Arc::new(AggregateHandler {
            cqrs,
            issuance_data_query,
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
        self.issuance_data_query.load(view_id).await
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
