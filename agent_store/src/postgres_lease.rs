//! The event-store writer lease on PostgreSQL.
//!
//! The lease is a session-level advisory lock, held on a dedicated connection for as long as the
//! process writes. Each acquisition increments the fencing token in `application_leases`. Appends
//! read the lease row `FOR SHARE` in their transaction, so an acquisition waits for appends in
//! flight, and appends after it fail for every process but the new holder.

use crate::writer_lease::{LeaseNotHeld, LEASE_ID};
use cqrs_es::{
    persist::{PersistedEventRepository, PersistenceError, ReplayStream, SerializedEvent, SerializedSnapshot},
    Aggregate,
};
use postgres_es::PostgresEventRepository;
use serde_json::Value;
use sqlx::{pool::PoolConnection, Pool, Postgres, Transaction};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Advisory lock key of the writer lease (`UNICORE` in ASCII).
const LEASE_LOCK_KEY: i64 = 0x55_4E_49_43_4F_52_45;
const MONITOR_INTERVAL: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) const CREATE_LEASE_TABLE: &str = "CREATE TABLE IF NOT EXISTS application_leases
(
    lease_id      text        NOT NULL,
    owner_id      text        NOT NULL,
    fencing_token bigint      NOT NULL,
    updated_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (lease_id)
)";

pub struct PostgresWriterLease {
    pool: Pool<Postgres>,
    owner_id: String,
    token: AtomicI64,
    active: AtomicBool,
    connection: tokio::sync::Mutex<Option<PoolConnection<Postgres>>>,
    stop: watch::Sender<bool>,
    lost: watch::Sender<bool>,
    monitor_task: Mutex<Option<JoinHandle<()>>>,
}

impl PostgresWriterLease {
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

    /// Waits until the advisory lock is free, takes it and increments the fencing token.
    pub(crate) async fn acquire(self: &Arc<Self>) -> Result<(), sqlx::Error> {
        sqlx::query(CREATE_LEASE_TABLE).execute(&self.pool).await?;

        let mut waiting_logged = false;
        loop {
            let mut connection = self.pool.acquire().await?;
            let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
                .bind(LEASE_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await?;
            if !acquired {
                drop(connection);
                if !waiting_logged {
                    info!("Waiting for the event-store writer lease held by another process");
                    waiting_logged = true;
                }
                tokio::time::sleep(RETRY_INTERVAL).await;
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
            info!(owner_id = %self.owner_id, fencing_token = token, "Acquired event-store writer lease");
            self.start_monitor();
            return Ok(());
        }
    }

    /// Checks the lease connection periodically: the advisory lock ends with its connection.
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
                            error!(error = %monitor_error, "Event-store writer lease connection lost");
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
        let _ = lost.wait_for(|lost| *lost).await;
    }

    pub(crate) async fn release(&self) {
        self.stop.send_replace(true);
        let task = self.monitor_task.lock().expect("lease task lock poisoned").take();
        if let Some(task) = task {
            let _ = task.await;
        }

        let held = self.active.swap(false, Ordering::AcqRel);
        let connection = self.connection.lock().await.take();
        if let Some(mut connection) = connection {
            if !held {
                // Lost: the advisory lock ended with its session, or belongs to another process.
                let _ = connection.close().await;
                return;
            }
            match sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
                .bind(LEASE_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
            {
                Ok(true) => info!(owner_id = %self.owner_id, "Released event-store writer lease"),
                Ok(false) => warn!(owner_id = %self.owner_id, "Event-store writer lease no longer held on release"),
                Err(release_error) => {
                    warn!(error = %release_error, "Failed to release event-store writer lease");
                    // The advisory lock ends with its session: close the connection instead.
                    let _ = connection.close().await;
                }
            }
        }
    }

    /// Fails unless this process holds the lease with its fencing token.
    async fn fence(&self, transaction: &mut Transaction<'_, Postgres>) -> Result<(), PersistenceError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(PersistenceError::UnknownError(Box::new(LeaseNotHeld)));
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
            error!(owner_id = %self.owner_id, "Event-store writer lease taken over by another process");
            self.mark_lost();
            return Err(PersistenceError::UnknownError(Box::new(LeaseNotHeld)));
        }
        Ok(())
    }
}

/// [`PostgresEventRepository`] with appends fenced by the writer lease. Reads are delegated.
pub(crate) struct LeasedPostgresEventRepository {
    repository: PostgresEventRepository,
    pool: Pool<Postgres>,
    lease: Arc<PostgresWriterLease>,
}

impl LeasedPostgresEventRepository {
    pub(crate) fn new(pool: Pool<Postgres>, lease: Arc<PostgresWriterLease>) -> Self {
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
                "snapshots are not supported by the leased PostgreSQL event repository".into(),
            ));
        }

        // Same statement as `postgres_es`, which offers no way to run it in a given transaction.
        let mut transaction = self.pool.begin().await.map_err(postgres_persistence_error)?;
        self.lease.fence(&mut transaction).await?;
        for event in events {
            let sequence =
                i64::try_from(event.sequence).map_err(|error| PersistenceError::UnknownError(error.into()))?;
            sqlx::query(
                "INSERT INTO events (aggregate_type, aggregate_id, sequence, event_type, event_version, payload, metadata)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
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

/// Maps errors like `postgres_es`: a duplicate sequence is an optimistic lock conflict.
fn postgres_persistence_error(error: sqlx::Error) -> PersistenceError {
    match &error {
        sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505") => {
            PersistenceError::OptimisticLockError
        }
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => PersistenceError::ConnectionError(Box::new(error)),
        _ => PersistenceError::UnknownError(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::{event_sink::EventSink, DomainEvent};
    use serde::{Deserialize, Serialize};
    use sqlx::postgres::PgPoolOptions;
    use std::convert::Infallible;

    #[derive(Default, Deserialize, Serialize)]
    struct TestAggregate;

    #[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
    struct TestEvent;

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> String {
            "WriterLeaseTested".to_string()
        }

        fn event_version(&self) -> String {
            "1".to_string()
        }
    }

    impl Aggregate for TestAggregate {
        const TYPE: &'static str = "writer_lease_test";
        type Command = Infallible;
        type Event = TestEvent;
        type Error = Infallible;
        type Services = ();

        async fn handle(
            &mut self,
            command: Self::Command,
            _: &Self::Services,
            _: &EventSink<Self>,
        ) -> Result<(), Self::Error> {
            match command {}
        }

        fn apply(&mut self, _event: Self::Event) {}
    }

    fn event(aggregate_id: &str, sequence: usize) -> SerializedEvent {
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

    /// Runs against the PostgreSQL database in `SSI_AGENT_TEST_POSTGRES_URI`, and is skipped without it.
    #[tokio::test]
    async fn a_writer_whose_lease_was_taken_over_cannot_append() {
        let Ok(connection_string) = std::env::var("SSI_AGENT_TEST_POSTGRES_URI") else {
            return;
        };
        let pool = PgPoolOptions::new().connect(&connection_string).await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS events
            (
                aggregate_type text                         NOT NULL,
                aggregate_id   text                         NOT NULL,
                sequence       bigint CHECK (sequence >= 0) NOT NULL,
                event_type     text                         NOT NULL,
                event_version  text                         NOT NULL,
                payload        json                         NOT NULL,
                metadata       json                         NOT NULL,
                PRIMARY KEY (aggregate_type, aggregate_id, sequence)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let aggregate_id = Uuid::new_v4().to_string();

        let first_lease = PostgresWriterLease::new(pool.clone());
        let first = LeasedPostgresEventRepository::new(pool.clone(), first_lease.clone());
        assert!(
            first
                .persist::<TestAggregate>(&[event(&aggregate_id, 1)], None)
                .await
                .is_err(),
            "appended before acquiring the lease"
        );
        first_lease.acquire().await.unwrap();
        first
            .persist::<TestAggregate>(&[event(&aggregate_id, 1)], None)
            .await
            .unwrap();

        let second_lease = PostgresWriterLease::new(pool.clone());
        let acquiring = second_lease.clone();
        let acquisition = tokio::spawn(async move { acquiring.acquire().await });
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!acquisition.is_finished(), "lease acquired while held");

        // The first writer's lease connection breaks, while its other connections keep working.
        sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_locks WHERE locktype = 'advisory' AND granted")
            .execute(&pool)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), acquisition)
            .await
            .expect("lease not acquired after its holder's connection broke")
            .unwrap()
            .unwrap();

        assert!(
            first
                .persist::<TestAggregate>(&[event(&aggregate_id, 2)], None)
                .await
                .is_err(),
            "appended after the lease was taken over"
        );
        tokio::time::timeout(Duration::from_secs(1), first_lease.lost())
            .await
            .expect("lease loss not reported");
        let second = LeasedPostgresEventRepository::new(pool.clone(), second_lease.clone());
        second
            .persist::<TestAggregate>(&[event(&aggregate_id, 2)], None)
            .await
            .unwrap();
        assert!(matches!(
            second.persist::<TestAggregate>(&[event(&aggregate_id, 2)], None).await,
            Err(PersistenceError::OptimisticLockError)
        ));

        second_lease.release().await;
        sqlx::query("DELETE FROM events WHERE aggregate_type = $1 AND aggregate_id = $2")
            .bind(TestAggregate::TYPE)
            .bind(&aggregate_id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
