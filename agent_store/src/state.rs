use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use std::sync::Arc;

use crate::config::config;
use crate::config::cqrs_framework;

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub cqrs: Arc<PostgresCqrs<A>>,
    pub issuance_data_query: Arc<PostgresViewRepository<V, A>>,
}

pub async fn application_state<A, V>(queries: Vec<Box<dyn Query<A>>>, services: A::Services) -> ApplicationState<A, V>
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
{
    let pool = default_postgress_pool(&config().get_string("db_connection_string").unwrap()).await;
    let (cqrs, issuance_data_query) = cqrs_framework(pool, queries, services);
    ApplicationState {
        cqrs,
        issuance_data_query,
    }
}
