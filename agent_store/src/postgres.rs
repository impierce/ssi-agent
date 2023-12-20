use agent_issuance::state::CQRS;
use agent_shared::config;
use async_trait::async_trait;
use cqrs_es::persist::{GenericQuery, PersistenceError, ViewRepository};
use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub cqrs: Arc<PostgresCqrs<A>>,
    pub issuance_data_query: Arc<PostgresViewRepository<V, A>>,
}

#[async_trait]
impl<A, V> CQRS<A, V> for ApplicationState<A, V>
where
    V: View<A> + 'static,
    A: Aggregate + 'static,
{
    async fn new(
        queries: Vec<Box<dyn Query<A>>>,
        services: A::Services,
    ) -> agent_issuance::state::ApplicationState<A, V>
    where
        Self: Sized,
    {
        let pool = default_postgress_pool(&config!("db_connection_string").unwrap()).await;
        let (cqrs, issuance_data_query) = cqrs_framework(pool, queries, services);
        Arc::new(ApplicationState {
            cqrs,
            issuance_data_query,
        }) as agent_issuance::state::ApplicationState<A, V>
    }

    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: A::Command,
        metadata: HashMap<String, String>,
    ) -> Result<(), cqrs_es::AggregateError<A::Error>>
    where
        A::Command: Send + Sync,
    {
        self.cqrs.execute_with_metadata(aggregate_id, command, metadata).await
    }

    async fn load(&self, view_id: &str) -> Result<Option<V>, PersistenceError> {
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
