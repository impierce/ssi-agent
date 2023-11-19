use std::sync::Arc;

use cqrs_es::{persist::GenericQuery, Aggregate, Query, View};
use postgres_es::{PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};

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
    let credential_view_repo = Arc::new(PostgresViewRepository::new(
        "credential_query",
        pool.clone(),
    ));
    let mut credential_query = GenericQuery::new(credential_view_repo.clone());
    credential_query.use_error_handler(Box::new(|e| println!("{}", e)));

    // Create and return an event-sourced `CqrsFramework`.
    queries.push(Box::new(credential_query));
    // let services = IssuanceServices {};
    (
        Arc::new(postgres_es::postgres_cqrs(pool, queries, services)),
        credential_view_repo,
    )
}
