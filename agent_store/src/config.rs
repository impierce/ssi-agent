use std::sync::Arc;

use cqrs_es::{Aggregate, Query};
use postgres_es::PostgresCqrs;
use sqlx::{Pool, Postgres};

// use crate::model::aggregate::Credential;
// use crate::queries::TempQuery;
// // use crate::queries::{AccountQuery, BankAccountView, SimpleLoggingQuery};
// use crate::services::IssuanceServices;

pub fn cqrs_framework<A, Q>(
    pool: Pool<Postgres>,
    query: Q,
    services: A::Services,
) -> Arc<PostgresCqrs<A>>
where
    A: Aggregate,
    Q: Query<A> + 'static, // Arc<PostgresViewRepository<TempQuery, Credential>>,
{
    // // A query that stores the current state of an individual account.
    // let account_view_repo = Arc::new(PostgresViewRepository::new("account_query", pool.clone()));
    // let mut account_query = AccountQuery::new(account_view_repo.clone());

    // // Without a query error handler there will be no indication if an
    // // error occurs (e.g., database connection failure, missing columns or table).
    // // Consider logging an error or panicking in your own application.
    // account_query.use_error_handler(Box::new(|e| println!("{}", e)));

    // Create and return an event-sourced `CqrsFramework`.
    let queries: Vec<Box<dyn Query<A>>> = vec![Box::new(query)];
    // let services = IssuanceServices {};
    Arc::new(postgres_es::postgres_cqrs(pool, queries, services))
    // account_view_repo,
}
