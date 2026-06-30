use crate::{AggregateHandler, CqrsComponentBuilder};
use agent_shared::{application_state::Command, config::config};
use cqrs_es::persist::PersistedEventStore;
use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresEventRepository, PostgresViewRepository};
use shared_kernel::view_repository::DynViewRepository;
use sqlx::Pool;
use std::sync::Arc;

impl<A> AggregateHandler<A, PersistedEventStore<PostgresEventRepository, A>>
where
    A: Aggregate,
{
    fn new(pool: Pool<sqlx::Postgres>, services: A::Services) -> Self {
        Self {
            cqrs: postgres_es::postgres_cqrs(pool, vec![], services),
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
            Arc::new(AggregateHandler::new(self.pool.clone(), services).with_parameters(
                aggregate.clone(),
                all_aggregates.clone(),
                event_publishers,
                &all_aggregates_name,
            )),
            aggregate,
            all_aggregates,
        )
    }
}
