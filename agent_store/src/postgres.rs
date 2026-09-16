use crate::event_verification::{self, EventVerificationError, EventVerificationReport, EventVerifier, RawStoredEvent};
use crate::{
    in_memory::InMemoryViewRepository,
    postgres_lease::{LeasedPostgresEventRepository, WriterLease},
    replay::{ReplayError, ReplayJob, ReplayProgress, ReplayProjection, ReplaySummary},
    AggregateHandler, CqrsComponentBuilder,
};
use agent_shared::{application_state::Command, config::config};
use cqrs_es::persist::PersistedEventStore;
use cqrs_es::{Aggregate, CqrsFramework, Query, View};
use futures::TryStreamExt;
use postgres_es::default_postgress_pool;
use shared_kernel::view_repository::DynViewRepository;
use sqlx::postgres::PgRow;
use sqlx::{Pool, Row};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

impl<A> AggregateHandler<A, PersistedEventStore<LeasedPostgresEventRepository, A>>
where
    A: Aggregate,
{
    fn new_leased(pool: Pool<sqlx::Postgres>, lease: Arc<WriterLease>, services: A::Services) -> Self {
        let repository = LeasedPostgresEventRepository::new(pool, lease);
        Self {
            cqrs: CqrsFramework::new(PersistedEventStore::new_event_store(repository), vec![], services),
            execution: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

pub struct Postgres {
    pub pool: Pool<sqlx::Postgres>,
    replay_jobs: Mutex<Vec<Arc<dyn ReplayJob>>>,
    writer_lease: Arc<WriterLease>,
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
        let writer_lease = WriterLease::new(pool.clone());
        Self {
            pool,
            replay_jobs: Mutex::new(Vec::new()),
            writer_lease,
        }
    }
    pub async fn verify_events(&self) -> Result<EventVerificationReport, EventVerificationError> {
        self.verify_events_with(event_verification::core_event_verifiers())
            .await
    }

    pub async fn acquire_writer_lease(&self) -> Result<(), sqlx::Error> {
        self.writer_lease.acquire().await
    }

    pub async fn writer_lease_lost(&self) {
        self.writer_lease.lost().await;
    }

    pub async fn shutdown(&self) {
        self.writer_lease.release().await;
        self.pool.close().await;
    }

    pub async fn replay_views(&self) -> Result<ReplaySummary, ReplayError> {
        let jobs = self.replay_jobs.lock().expect("replay jobs lock poisoned").clone();
        let jobs_by_type = jobs
            .iter()
            .map(|job| (job.aggregate_type(), job.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut progress = jobs
            .iter()
            .map(|job| (job.aggregate_type(), ReplayProgress::default()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut skipped: BTreeMap<String, usize> = BTreeMap::new();

        let mut rows = sqlx::query(
            "SELECT aggregate_type, aggregate_id, sequence, event_type, event_version, payload
             FROM events
             ORDER BY aggregate_type, aggregate_id, sequence",
        )
        .fetch(&self.pool);
        while let Some(row) = rows.try_next().await.map_err(EventVerificationError::from)? {
            let raw = raw_stored_event(row);
            let Some(job) = crate::replay::resolve_job(&jobs_by_type, &raw.aggregate_type, &mut skipped) else {
                continue;
            };
            progress
                .get_mut(job.aggregate_type())
                .expect("registered replay job has progress")
                .apply(job.as_ref(), raw)
                .await?;
        }

        let mut reports = Vec::with_capacity(jobs.len());
        for job in jobs {
            reports.push(
                progress
                    .remove(job.aggregate_type())
                    .expect("registered replay job has progress")
                    .finish(job.aggregate_type())?,
            );
        }
        Ok(ReplaySummary { reports, skipped })
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

        Ok(rows.into_iter().map(raw_stored_event).collect())
    }
}

fn raw_stored_event(row: PgRow) -> RawStoredEvent {
    RawStoredEvent {
        aggregate_type: row.get("aggregate_type"),
        aggregate_id: row.get("aggregate_id"),
        sequence: row.get("sequence"),
        event_type: row.get("event_type"),
        event_version: row.get("event_version"),
        payload: row.get("payload"),
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

        let aggregate: Arc<InMemoryViewRepository<V, A>> = Arc::new(InMemoryViewRepository::default());
        let all_aggregates: Arc<InMemoryViewRepository<AV, A>> = Arc::new(InMemoryViewRepository::default());
        self.replay_jobs
            .lock()
            .expect("replay jobs lock poisoned")
            .push(Arc::new(ReplayProjection::new(
                aggregate.clone(),
                all_aggregates.clone(),
                all_aggregates_name.clone(),
            )));

        (
            Arc::new(
                AggregateHandler::new_leased(self.pool.clone(), self.writer_lease.clone(), services).with_parameters(
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
