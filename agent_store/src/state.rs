// use crate::config::cqrs_framework;
// use crate::model::aggregate::Credential;
// use crate::queries::TempQuery;
use cqrs_es::{Aggregate, Query};
use postgres_es::{default_postgress_pool, PostgresCqrs};
use std::sync::Arc;

use crate::config::cqrs_framework;

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate> {
    pub cqrs: Arc<PostgresCqrs<A>>,
}

pub async fn application_state<A, Q>(query: Q, services: A::Services) -> ApplicationState<A>
where
    A: Aggregate,
    Q: Query<A> + 'static,
{
    let pool = default_postgress_pool("postgresql://demo_user:demo_pass@localhost:5432/demo").await;
    let cqrs = cqrs_framework(pool, query, services);
    // cqrs
    ApplicationState { cqrs }
}
