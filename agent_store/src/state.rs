use cqrs_es::{mem_store::MemStore, persist::PersistedEventStore, Aggregate, CqrsFramework, EventStore, Query, View};
use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresEventRepository, PostgresViewRepository};
use std::sync::Arc;

use crate::config::cqrs_framework;

// #[derive(Clone)]
// pub struct ApplicationState<A: Aggregate, V: View<A>> {
//     pub cqrs: Arc<PostgresCqrs<A>>,
//     pub credential_query: Arc<PostgresViewRepository<V, A>>,
// }

#[derive(Clone)]
pub struct ApplicationState<A: Aggregate, V: View<A>> {
    pub cqrs: Arc<StoreType<A>>,
    pub credential_query: Arc<PostgresViewRepository<V, A>>,
}

pub async fn application_state<A, V>(
    queries: Vec<Box<dyn Query<A>>>,
    services: A::Services,
    in_mem: bool,
) -> ApplicationState<A, V>
where
    A: Aggregate + 'static,
    V: View<A> + 'static,
{
    if in_mem {
        ApplicationState {
            cqrs: Arc::new(CqrsFramework::new(MemStore::<A>::default(), queries, services)),
            credential_query: Arc::new(PostgresViewRepository::new()),
        }
    } else {
        let pool = default_postgress_pool("postgresql://demo_user:demo_pass@localhost:5432/demo").await;
        let (cqrs, credential_query) = cqrs_framework(pool, queries, services);
        ApplicationState { cqrs, credential_query }
    }
}

enum StoreType<A: Aggregate> {
    InMem(MemStore<A>),
    Postgres(PostgresCqrs<A>),
}
