use crate::event_verification::{self, EventVerificationError, EventVerificationReport, EventVerifier, RawStoredEvent};
use crate::{
    custom_queries::{modify_via_store, MutableViewRepository},
    AggregateHandler, CqrsComponentBuilder,
};
use agent_shared::{application_state::Command, config::config};
use async_trait::async_trait;
use cqrs_es::persist::{PersistedEventStore, PersistenceError};
use cqrs_es::{Aggregate, Query, View};
use postgres_es::{default_postgress_pool, PostgresEventRepository, PostgresViewRepository};
use shared_kernel::view_repository::DynViewRepository;
use sqlx::{Pool, Row};
use std::sync::Arc;

#[async_trait]
impl<V, A> MutableViewRepository<V, A> for PostgresViewRepository<V, A>
where
    V: View<A>,
    A: Aggregate,
{
    async fn modify(
        &self,
        view_id: &str,
        update: &mut (dyn for<'view> FnMut(&'view mut V) + Send),
    ) -> Result<(), PersistenceError> {
        modify_via_store(self, view_id, update).await
    }
}

impl<A> AggregateHandler<A, PersistedEventStore<PostgresEventRepository, A>>
where
    A: Aggregate,
{
    fn new(pool: Pool<sqlx::Postgres>, services: A::Services) -> Self {
        Self {
            cqrs: postgres_es::postgres_cqrs(pool, vec![], services),
            execution: Arc::new(tokio::sync::Mutex::new(())),
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
        Self::connect(&connection_string).await
    }

    async fn connect(connection_string: &str) -> Self {
        let pool = default_postgress_pool(connection_string).await;
        Self { pool }
    }
    // TODO: Run [Pool::close] during graceful shutdown to close all open connections.

    pub async fn verify_events(&self) -> Result<EventVerificationReport, EventVerificationError> {
        self.verify_events_with(event_verification::core_event_verifiers())
            .await
    }

    pub async fn verify_events_with(
        &self,
        verifiers: &[EventVerifier],
    ) -> Result<EventVerificationReport, EventVerificationError> {
        Ok(event_verification::verify_events_with(
            self.load_raw_events().await?,
            verifiers,
        ))
    }

    pub async fn load_raw_events(&self) -> Result<Vec<RawStoredEvent>, EventVerificationError> {
        let rows = sqlx::query(
            "SELECT aggregate_type, aggregate_id, sequence, event_type, event_version, payload
          FROM events
          ORDER BY aggregate_type, aggregate_id, sequence",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| RawStoredEvent {
                aggregate_type: row.get("aggregate_type"),
                aggregate_id: row.get("aggregate_id"),
                sequence: row.get("sequence"),
                event_type: row.get("event_type"),
                event_version: row.get("event_version"),
                payload: row.get("payload"),
            })
            .collect())
    }
}

impl CqrsComponentBuilder for Postgres {
    async fn commands_and_queries<V: View<A> + Clone + 'static, A: Aggregate + 'static, AV: View<A> + Clone + 'static>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "test".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("test aggregate error")]
    struct TestError;

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "postgres_mutable_view_test";
        type Command = ();
        type Event = TestEvent;
        type Error = TestError;
        type Services = ();

        async fn handle(
            &mut self,
            _command: Self::Command,
            _service: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    struct TestView(usize);

    impl View<TestAggregate> for TestView {
        fn update(&mut self, _event: &cqrs_es::EventEnvelope<TestAggregate>) {
            self.0 += 1;
        }
    }

    #[tokio::test]
    async fn mutable_view_round_trips_through_postgres() {
        let Ok(connection_string) = std::env::var("SSI_AGENT_TEST_POSTGRES_URI") else {
            return;
        };
        let store = Postgres::connect(&connection_string).await;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS postgres_mutable_view_test (
                view_id text PRIMARY KEY,
                version bigint NOT NULL CHECK (version >= 0),
                payload json NOT NULL
            )",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query("TRUNCATE TABLE postgres_mutable_view_test")
            .execute(&store.pool)
            .await
            .unwrap();

        let repository =
            PostgresViewRepository::<TestView, TestAggregate>::new("postgres_mutable_view_test", store.pool.clone());
        repository.modify("all", &mut |view| view.0 += 3).await.unwrap();

        assert_eq!(repository.load("all").await.unwrap().unwrap().0, 3);
        store.pool.close().await;
    }
}
