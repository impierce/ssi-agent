use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::{application_state::Command, config::config};
use cqrs_es::persist::{EventUpcaster, PersistedEventStore};
use cqrs_es::{Aggregate, CqrsFramework, Query, View};
use postgres_es::{default_postgress_pool, PostgresEventRepository, PostgresViewRepository};
use shared_kernel::view_repository::DynViewRepository;
use sqlx::Pool;
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<PostgresEventRepository, A>>
where
    A: Aggregate,
{
    fn new(pool: Pool<sqlx::Postgres>, services: A::Services, upcasters: Vec<Box<dyn EventUpcaster>>) -> Self {
        // Mirrors `postgres_es::postgres_cqrs`, but threads through the per-aggregate
        // `upcasters` which that convenience function does not accept.
        let repo = PostgresEventRepository::new(pool);
        let store = PersistedEventStore::new_event_store(repo).with_upcasters(upcasters);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
        }
    }
}

pub struct Postgres {
    pub pool: Pool<sqlx::Postgres>,
}

impl Postgres {
    pub async fn new() -> Self {
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let pool = default_postgress_pool(&connection_string).await;
        Self { pool }
    }
    // TODO: Run [Pool::close] during graceful shutdown to close all open connections.
}

impl CqrsComponentBuilder for Postgres {
    async fn commands_and_queries<V: View<A> + 'static, A: Aggregate + 'static, AV: View<A> + 'static>(
        &self,
        services: A::Services,
        event_publishers: Vec<Box<dyn Query<A>>>,
        upcasters: Vec<Box<dyn EventUpcaster>>,
    ) -> (
        Arc<dyn Command<A> + Send + Sync>,
        Arc<dyn DynViewRepository<V, A>>,
        Arc<dyn DynViewRepository<AV, A>>,
    )
    where
        <A as Aggregate>::Command: Send + Sync,
    {
        let all_aggregates_name = format!("all_{}s", A::TYPE);

        // Initialize the postgres repositories.
        let aggregate: Arc<PostgresViewRepository<V, A>> =
            Arc::new(PostgresViewRepository::<V, A>::new(A::TYPE, self.pool.clone()));
        let all_aggregates: Arc<PostgresViewRepository<AV, A>> = Arc::new(PostgresViewRepository::<AV, A>::new(
            &all_aggregates_name,
            self.pool.clone(),
        ));

        (
            Arc::new(
                AggregateHandler::new(self.pool.clone(), services, upcasters).with_parameters(
                    aggregate.clone(),
                    all_aggregates.clone(),
                    event_publishers,
                    &all_aggregates_name,
                ),
            ),
            aggregate,
            all_aggregates,
        )
    }
}
