use cqrs_es::{
    persist::{PersistedEventRepository, PersistenceError, ReplayStream, SerializedEvent, SerializedSnapshot},
    Aggregate,
};
use postgres_es::PostgresEventRepository;
use serde_json::Value;
use sqlx::{pool::PoolConnection, Pool, Postgres, Transaction};
use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};
use uuid::Uuid;

const LEASE_ID: &str = "in_memory_projection_writer";
const LEASE_LOCK_KEY: i64 = 0x55_4E_49_43_4F_52_45;
const MONITOR_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct LeaseLost;

impl fmt::Display for LeaseLost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Postgres in-memory projection writer lease is not active")
    }
}

impl Error for LeaseLost {}

pub(crate) struct WriterLease {
    pool: Pool<Postgres>,
    owner_id: String,
    token: AtomicI64,
    active: AtomicBool,
    connection: tokio::sync::Mutex<Option<PoolConnection<Postgres>>>,
    stop: watch::Sender<bool>,
    lost: watch::Sender<bool>,
    monitor_task: Mutex<Option<JoinHandle<()>>>,
}

impl WriterLease {
    pub(crate) fn new(pool: Pool<Postgres>) -> Arc<Self> {
        let (stop, _) = watch::channel(false);
        let (lost, _) = watch::channel(false);
        Arc::new(Self {
            pool,
            owner_id: Uuid::new_v4().to_string(),
            token: AtomicI64::new(0),
            active: AtomicBool::new(false),
            connection: tokio::sync::Mutex::new(None),
            stop,
            lost,
            monitor_task: Mutex::new(None),
        })
    }

    pub(crate) async fn acquire(self: &Arc<Self>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS application_leases (
                lease_id text PRIMARY KEY,
                owner_id text NOT NULL,
                fencing_token bigint NOT NULL,
                updated_at timestamptz NOT NULL DEFAULT now()
            )",
        )
        .execute(&self.pool)
        .await?;

        loop {
            let mut connection = self.pool.acquire().await?;
            let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
                .bind(LEASE_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await?;
            if !acquired {
                drop(connection);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }

            let token: i64 = sqlx::query_scalar(
                "INSERT INTO application_leases (lease_id, owner_id, fencing_token)
                 VALUES ($1, $2, 1)
                 ON CONFLICT (lease_id) DO UPDATE
                 SET owner_id = EXCLUDED.owner_id,
                     fencing_token = application_leases.fencing_token + 1,
                     updated_at = now()
                 RETURNING fencing_token",
            )
            .bind(LEASE_ID)
            .bind(&self.owner_id)
            .fetch_one(&mut *connection)
            .await?;

            self.token.store(token, Ordering::Release);
            self.active.store(true, Ordering::Release);
            self.lost.send_replace(false);
            *self.connection.lock().await = Some(connection);
            info!(owner_id = %self.owner_id, fencing_token = token, "Acquired Postgres writer lease");
            self.start_monitor();
            return Ok(());
        }
    }

    fn start_monitor(self: &Arc<Self>) {
        let lease = Arc::downgrade(self);
        let mut stop = self.stop.subscribe();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = stop.changed() => {
                        if changed.is_err() || *stop.borrow() {
                            return;
                        }
                    }
                    () = tokio::time::sleep(MONITOR_INTERVAL) => {
                        let Some(lease) = lease.upgrade() else {
                            return;
                        };
                        let mut connection = lease.connection.lock().await;
                        let Some(connection) = connection.as_mut() else {
                            lease.mark_lost();
                            return;
                        };
                        if let Err(monitor_error) = sqlx::query("SELECT 1").execute(&mut **connection).await {
                            error!(error = %monitor_error, "Postgres writer lease connection was lost");
                            lease.mark_lost();
                            return;
                        }
                    }
                }
            }
        });
        *self.monitor_task.lock().expect("lease task lock poisoned") = Some(task);
    }

    fn mark_lost(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            self.lost.send_replace(true);
        }
    }

    pub(crate) async fn lost(&self) {
        let mut lost = self.lost.subscribe();
        if *lost.borrow() {
            return;
        }
        while lost.changed().await.is_ok() {
            if *lost.borrow() {
                return;
            }
        }
    }

    pub(crate) async fn release(&self) {
        self.stop.send_replace(true);
        let task = self.monitor_task.lock().expect("lease task lock poisoned").take();
        if let Some(task) = task {
            let _ = task.await;
        }

        let mut connection = self.connection.lock().await;
        if let Some(mut connection) = connection.take() {
            match sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                .bind(LEASE_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
            {
                Ok(true) => info!(owner_id = %self.owner_id, "Released Postgres writer lease"),
                Ok(false) => {
                    warn!(owner_id = %self.owner_id, "Postgres writer lease was no longer owned during release")
                }
                Err(release_error) => warn!(error = %release_error, "Failed to release Postgres writer lease"),
            }
        }
        self.active.store(false, Ordering::Release);
    }

    async fn fence(&self, transaction: &mut Transaction<'_, Postgres>) -> Result<(), PersistenceError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(PersistenceError::UnknownError(Box::new(LeaseLost)));
        }

        let current: Option<(String, i64)> = sqlx::query_as(
            "SELECT owner_id, fencing_token
             FROM application_leases
             WHERE lease_id = $1
             FOR SHARE",
        )
        .bind(LEASE_ID)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(postgres_persistence_error)?;

        let expected_token = self.token.load(Ordering::Acquire);
        if !matches!(current, Some((ref owner_id, token)) if owner_id == &self.owner_id && token == expected_token) {
            self.mark_lost();
            return Err(PersistenceError::UnknownError(Box::new(LeaseLost)));
        }
        Ok(())
    }
}

pub(crate) struct LeasedPostgresEventRepository {
    repository: PostgresEventRepository,
    pool: Pool<Postgres>,
    lease: Arc<WriterLease>,
}

impl LeasedPostgresEventRepository {
    pub(crate) fn new(pool: Pool<Postgres>, lease: Arc<WriterLease>) -> Self {
        Self {
            repository: PostgresEventRepository::new(pool.clone()),
            pool,
            lease,
        }
    }
}

impl PersistedEventRepository for LeasedPostgresEventRepository {
    async fn get_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<Vec<SerializedEvent>, PersistenceError> {
        self.repository.get_events::<A>(aggregate_id).await
    }

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
        last_sequence: usize,
    ) -> Result<Vec<SerializedEvent>, PersistenceError> {
        self.repository.get_last_events::<A>(aggregate_id, last_sequence).await
    }

    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &str,
    ) -> Result<Option<SerializedSnapshot>, PersistenceError> {
        self.repository.get_snapshot::<A>(aggregate_id).await
    }

    async fn persist<A: Aggregate>(
        &self,
        events: &[SerializedEvent],
        snapshot_update: Option<(String, Value, usize)>,
    ) -> Result<(), PersistenceError> {
        if snapshot_update.is_some() {
            return Err(PersistenceError::UnknownError(
                "snapshots are not supported by the leased Postgres event repository".into(),
            ));
        }

        let mut transaction = self.pool.begin().await.map_err(postgres_persistence_error)?;
        self.lease.fence(&mut transaction).await?;
        for event in events {
            let sequence = i64::try_from(event.sequence).map_err(conversion_persistence_error)?;
            sqlx::query(
                "INSERT INTO events (
                    aggregate_type,
                    aggregate_id,
                    sequence,
                    event_type,
                    event_version,
                    payload,
                    metadata
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(A::TYPE)
            .bind(&event.aggregate_id)
            .bind(sequence)
            .bind(&event.event_type)
            .bind(&event.event_version)
            .bind(&event.payload)
            .bind(&event.metadata)
            .execute(&mut *transaction)
            .await
            .map_err(postgres_persistence_error)?;
        }
        transaction.commit().await.map_err(postgres_persistence_error)
    }

    async fn stream_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<ReplayStream, PersistenceError> {
        self.repository.stream_events::<A>(aggregate_id).await
    }

    async fn stream_all_events<A: Aggregate>(&self) -> Result<ReplayStream, PersistenceError> {
        self.repository.stream_all_events::<A>().await
    }
}

fn postgres_persistence_error(error: sqlx::Error) -> PersistenceError {
    match &error {
        sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505") => {
            PersistenceError::OptimisticLockError
        }
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => PersistenceError::ConnectionError(Box::new(error)),
        _ => PersistenceError::UnknownError(Box::new(error)),
    }
}

fn conversion_persistence_error(error: impl Error + Send + Sync + 'static) -> PersistenceError {
    PersistenceError::UnknownError(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use postgres_es::default_postgress_pool;
    use serde::{Deserialize, Serialize};
    use std::convert::Infallible;

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "writer_lease_tested".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "postgres_writer_lease_test";
        type Command = Infallible;
        type Event = TestEvent;
        type Error = Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _service: &Self::Services,
            _sink: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    fn serialized_event(aggregate_id: &str, sequence: usize) -> SerializedEvent {
        SerializedEvent::new(
            aggregate_id.to_string(),
            sequence,
            TestAggregate::TYPE.to_string(),
            TestEvent.event_type(),
            TestEvent.event_version(),
            serde_json::to_value(TestEvent).unwrap(),
            serde_json::json!({}),
        )
    }

    #[tokio::test]
    async fn lease_handoff_fences_the_previous_writer() {
        let Ok(connection_string) = std::env::var("SSI_AGENT_TEST_POSTGRES_URI") else {
            return;
        };
        let pool = default_postgress_pool(&connection_string).await;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS events (
                aggregate_type text NOT NULL,
                aggregate_id text NOT NULL,
                sequence bigint NOT NULL,
                event_type text NOT NULL,
                event_version text NOT NULL,
                payload json NOT NULL,
                metadata json NOT NULL,
                PRIMARY KEY (aggregate_type, aggregate_id, sequence)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let aggregate_id = Uuid::new_v4().to_string();
        let first_lease = WriterLease::new(pool.clone());
        first_lease.acquire().await.unwrap();
        let first_repository = LeasedPostgresEventRepository::new(pool.clone(), first_lease.clone());
        first_repository
            .persist::<TestAggregate>(&[serialized_event(&aggregate_id, 1)], None)
            .await
            .unwrap();

        let second_lease = WriterLease::new(pool.clone());
        let acquiring_lease = second_lease.clone();
        let acquisition = tokio::spawn(async move { acquiring_lease.acquire().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!acquisition.is_finished());

        first_lease.release().await;
        tokio::time::timeout(Duration::from_secs(3), acquisition)
            .await
            .expect("second writer did not acquire the released lease")
            .unwrap()
            .unwrap();

        assert!(first_repository
            .persist::<TestAggregate>(&[serialized_event(&aggregate_id, 2)], None)
            .await
            .is_err());
        let second_repository = LeasedPostgresEventRepository::new(pool.clone(), second_lease.clone());
        second_repository
            .persist::<TestAggregate>(&[serialized_event(&aggregate_id, 2)], None)
            .await
            .unwrap();

        sqlx::query("DELETE FROM events WHERE aggregate_type = $1 AND aggregate_id = $2")
            .bind(TestAggregate::TYPE)
            .bind(&aggregate_id)
            .execute(&pool)
            .await
            .unwrap();
        second_lease.release().await;
        pool.close().await;
    }
}
