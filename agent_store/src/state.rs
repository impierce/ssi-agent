// use crate::config::cqrs_framework;
// use crate::model::aggregate::Credential;
// use crate::queries::SimpleLoggingQuery;
use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use std::sync::Arc;

use crate::config::cqrs_framework;

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub cqrs: Arc<PostgresCqrs<A>>,
    pub credential_query: Arc<PostgresViewRepository<V, A>>,
}

pub async fn application_state<A, V>(queries: Vec<Box<dyn Query<A>>>, services: A::Services) -> ApplicationState<A, V>
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
{
    let pool = default_postgress_pool("postgresql://demo_user:demo_pass@localhost:5432/demo").await;
    let (cqrs, credential_query) = cqrs_framework(pool, queries, services);
    // cqrs
    ApplicationState { cqrs, credential_query }
}
