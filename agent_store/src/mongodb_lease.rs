//! The event-store writer lease on MongoDB.
//!
//! The lease is a document in `application_leases` with an owner, a fencing token and an expiry,
//! renewed periodically. A process takes it over once it has expired, incrementing the fencing
//! token. Appends check the lease document in their transaction. The holder also stops appending on
//! its own once it could not renew for longer than `ttl - renew_interval`, which leaves appends in
//! flight `renew_interval` to commit before another process can take over.

use crate::writer_lease::{LeaseNotHeld, LEASE_ID};
use cqrs_es::{
    persist::{PersistedEventRepository, PersistenceError, ReplayStream, SerializedEvent, SerializedSnapshot},
    Aggregate,
};
use mongo_es::MongoEventRepository;
use mongodb::{
    bson::{self, doc, DateTime, Document},
    error::{Error as MongoError, ErrorKind, WriteFailure},
    options::ReturnDocument,
    Client, ClientSession, Collection,
};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Collection of `mongo_es`'s [`MongoEventRepository`].
const EVENT_COLLECTION: &str = "events";
const LEASE_COLLECTION: &str = "application_leases";
const DUPLICATE_KEY: i32 = 11000;

#[derive(Clone, Copy, Debug)]
pub(crate) struct WriterLeaseOptions {
    pub(crate) ttl: Duration,
    pub(crate) renew_interval: Duration,
    pub(crate) retry_interval: Duration,
}

impl Default for WriterLeaseOptions {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
            renew_interval: Duration::from_secs(10),
            retry_interval: Duration::from_secs(1),
        }
    }
}

pub struct MongoWriterLease {
    client: Client,
    owner_id: String,
    token: AtomicI64,
    active: AtomicBool,
    /// Instant after which this process no longer appends without a successful renewal.
    valid_until: Mutex<Instant>,
    options: WriterLeaseOptions,
    stop: watch::Sender<bool>,
    lost: watch::Sender<bool>,
    renewal_task: Mutex<Option<JoinHandle<()>>>,
}

impl MongoWriterLease {
    pub(crate) fn new(client: Client) -> Arc<Self> {
        Self::with_options(client, WriterLeaseOptions::default())
    }

    pub(crate) fn with_options(client: Client, options: WriterLeaseOptions) -> Arc<Self> {
        assert!(
            options.renew_interval < options.ttl,
            "lease must renew before it expires"
        );
        let (stop, _) = watch::channel(false);
        let (lost, _) = watch::channel(false);
        Arc::new(Self {
            client,
            owner_id: Uuid::new_v4().to_string(),
            token: AtomicI64::new(0),
            active: AtomicBool::new(false),
            valid_until: Mutex::new(Instant::now()),
            options,
            stop,
            lost,
            renewal_task: Mutex::new(None),
        })
    }

    fn collection(&self) -> Result<Collection<Document>, MongoError> {
        self.client
            .default_database()
            .map(|database| database.collection(LEASE_COLLECTION))
            .ok_or_else(|| MongoError::custom("MongoDB connection string names no database"))
    }

    /// Extends the local validity, measured from the start of the request that renewed the lease.
    fn extend_validity(&self, requested_at: Instant) {
        *self.valid_until.lock().expect("lease validity lock poisoned") =
            requested_at + (self.options.ttl - self.options.renew_interval);
    }

    fn is_valid(&self) -> bool {
        self.active.load(Ordering::Acquire)
            && Instant::now() < *self.valid_until.lock().expect("lease validity lock poisoned")
    }

    /// Waits until the lease is free or expired, takes it over and increments the fencing token.
    pub(crate) async fn acquire(self: &Arc<Self>) -> Result<(), MongoError> {
        let collection = self.collection()?;
        let created = collection
            .update_one(
                doc! { "_id": LEASE_ID },
                doc! {
                    "$setOnInsert": {
                        "owner_id": "",
                        "fencing_token": 0_i64,
                        "expires_at": DateTime::from_millis(0),
                    }
                },
            )
            .upsert(true)
            .await;
        match created {
            Ok(_) => {}
            // Another process created the document at the same time.
            Err(error) if is_duplicate_key(&error) => {}
            Err(error) => return Err(error),
        }

        let mut waiting_logged = false;
        loop {
            let requested_at = Instant::now();
            let lease = collection
                .find_one_and_update(
                    doc! {
                        "_id": LEASE_ID,
                        "$expr": { "$lte": ["$expires_at", "$$NOW"] },
                    },
                    vec![doc! {
                        "$set": {
                            "owner_id": &self.owner_id,
                            "fencing_token": { "$add": [{ "$ifNull": ["$fencing_token", 0_i64] }, 1_i64] },
                            "expires_at": lease_expiry(self.options.ttl),
                            "updated_at": "$$NOW",
                        }
                    }],
                )
                .return_document(ReturnDocument::After)
                .await?;

            if let Some(lease) = lease {
                let token = lease.get_i64("fencing_token").map_err(MongoError::custom)?;
                self.token.store(token, Ordering::Release);
                self.extend_validity(requested_at);
                self.active.store(true, Ordering::Release);
                self.lost.send_replace(false);
                info!(owner_id = %self.owner_id, fencing_token = token, "Acquired event-store writer lease");
                self.start_renewal();
                return Ok(());
            }

            if !waiting_logged {
                info!("Waiting for the event-store writer lease held by another process");
                waiting_logged = true;
            }
            tokio::time::sleep(self.options.retry_interval).await;
        }
    }

    fn start_renewal(self: &Arc<Self>) {
        let lease = Arc::downgrade(self);
        let options = self.options;
        let mut stop = self.stop.subscribe();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = stop.changed() => {
                        if changed.is_err() || *stop.borrow() {
                            return;
                        }
                    }
                    () = tokio::time::sleep(options.renew_interval) => {
                        let Some(lease) = lease.upgrade() else {
                            return;
                        };
                        let requested_at = Instant::now();
                        match lease.renew().await {
                            Ok(true) => lease.extend_validity(requested_at),
                            Ok(false) => {
                                error!(owner_id = %lease.owner_id, "Event-store writer lease taken over by another process");
                                lease.mark_lost();
                                return;
                            }
                            Err(renewal_error) if lease.is_valid() => {
                                warn!(error = %renewal_error, "Failed to renew event-store writer lease; retrying");
                            }
                            Err(renewal_error) => {
                                error!(error = %renewal_error, "Event-store writer lease expired without renewal");
                                lease.mark_lost();
                                return;
                            }
                        }
                    }
                }
            }
        });
        *self.renewal_task.lock().expect("lease task lock poisoned") = Some(task);
    }

    async fn renew(&self) -> Result<bool, MongoError> {
        let result = self
            .collection()?
            .update_one(
                doc! {
                    "_id": LEASE_ID,
                    "owner_id": &self.owner_id,
                    "fencing_token": self.token.load(Ordering::Acquire),
                },
                vec![doc! {
                    "$set": {
                        "expires_at": lease_expiry(self.options.ttl),
                        "updated_at": "$$NOW",
                    }
                }],
            )
            .await?;
        Ok(result.matched_count == 1)
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
        let task = self.renewal_task.lock().expect("lease task lock poisoned").take();
        if let Some(task) = task {
            let _ = task.await;
        }

        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        let released = match self.collection() {
            Ok(collection) => {
                collection
                    .update_one(
                        doc! {
                            "_id": LEASE_ID,
                            "owner_id": &self.owner_id,
                            "fencing_token": self.token.load(Ordering::Acquire),
                        },
                        vec![doc! {
                            "$set": {
                                "owner_id": "",
                                "expires_at": DateTime::from_millis(0),
                                "updated_at": "$$NOW",
                            }
                        }],
                    )
                    .await
            }
            Err(error) => Err(error),
        };
        match released {
            Ok(result) if result.matched_count == 1 => {
                info!(owner_id = %self.owner_id, "Released event-store writer lease");
            }
            Ok(_) => warn!(owner_id = %self.owner_id, "Event-store writer lease no longer held on release"),
            Err(release_error) => warn!(error = %release_error, "Failed to release event-store writer lease"),
        }
    }

    /// Fails unless this process holds the lease with its fencing token, and the lease is unexpired.
    async fn fence(&self, session: &mut ClientSession) -> Result<(), PersistenceError> {
        if !self.is_valid() {
            if self.active.load(Ordering::Acquire) {
                error!(owner_id = %self.owner_id, "Event-store writer lease expired without renewal");
            }
            self.mark_lost();
            return Err(PersistenceError::UnknownError(Box::new(LeaseNotHeld)));
        }

        let current = self
            .collection()
            .map_err(mongo_persistence_error)?
            .find_one(doc! {
                "_id": LEASE_ID,
                "owner_id": &self.owner_id,
                "fencing_token": self.token.load(Ordering::Acquire),
                "$expr": { "$gt": ["$expires_at", "$$NOW"] },
            })
            .session(session)
            .await
            .map_err(mongo_persistence_error)?;
        if current.is_none() {
            error!(owner_id = %self.owner_id, "Event-store writer lease taken over by another process");
            self.mark_lost();
            return Err(PersistenceError::UnknownError(Box::new(LeaseNotHeld)));
        }
        Ok(())
    }
}

fn lease_expiry(ttl: Duration) -> Document {
    doc! {
        "$dateAdd": {
            "startDate": "$$NOW",
            "unit": "millisecond",
            "amount": i64::try_from(ttl.as_millis()).expect("lease duration exceeds i64 milliseconds"),
        }
    }
}

/// [`MongoEventRepository`] with appends fenced by the writer lease. Reads are delegated.
pub(crate) struct LeasedMongoEventRepository {
    repository: MongoEventRepository,
    client: Client,
    lease: Arc<MongoWriterLease>,
}

impl LeasedMongoEventRepository {
    pub(crate) fn new(repository: MongoEventRepository, client: Client, lease: Arc<MongoWriterLease>) -> Self {
        Self {
            repository,
            client,
            lease,
        }
    }

    fn events(&self) -> Collection<Document> {
        self.client
            .default_database()
            .expect("MongoDB connection string must name a database")
            .collection(EVENT_COLLECTION)
    }
}

impl PersistedEventRepository for LeasedMongoEventRepository {
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
                "snapshots are not supported by the leased MongoDB event repository".into(),
            ));
        }
        if events.is_empty() {
            return Ok(());
        }
        let documents = event_documents(events)?;

        let mut session = self.client.start_session().await.map_err(mongo_persistence_error)?;
        session.start_transaction().await.map_err(mongo_persistence_error)?;
        let result = async {
            self.lease.fence(&mut session).await?;
            self.events()
                .insert_many(documents)
                .session(&mut session)
                .await
                .map_err(mongo_persistence_error)?;
            session.commit_transaction().await.map_err(mongo_persistence_error)
        }
        .await;
        if result.is_err() {
            let _ = session.abort_transaction().await;
        }
        result
    }

    async fn stream_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<ReplayStream, PersistenceError> {
        self.repository.stream_events::<A>(aggregate_id).await
    }

    async fn stream_all_events<A: Aggregate>(&self) -> Result<ReplayStream, PersistenceError> {
        self.repository.stream_all_events::<A>().await
    }
}

/// Same document layout as `mongo_es`, which offers no way to insert in a given session.
fn event_documents(events: &[SerializedEvent]) -> Result<Vec<Document>, PersistenceError> {
    events
        .iter()
        .map(|event| {
            Ok(doc! {
                "aggregate_id": &event.aggregate_id,
                "aggregate_type": &event.aggregate_type,
                "sequence": i64::try_from(event.sequence).map_err(|error| PersistenceError::UnknownError(error.into()))?,
                "event_type": &event.event_type,
                "event_version": &event.event_version,
                "payload": bson::to_bson(&event.payload).map_err(|error| PersistenceError::UnknownError(error.into()))?,
                "metadata": bson::to_bson(&event.metadata).map_err(|error| PersistenceError::UnknownError(error.into()))?,
            })
        })
        .collect()
}

fn is_duplicate_key(error: &MongoError) -> bool {
    match error.kind.as_ref() {
        ErrorKind::InsertMany(insert_error) => insert_error
            .write_errors
            .iter()
            .flatten()
            .any(|write_error| write_error.code == DUPLICATE_KEY),
        ErrorKind::Write(WriteFailure::WriteError(write_error)) => write_error.code == DUPLICATE_KEY,
        _ => false,
    }
}

/// Maps errors like `mongo_es`: a duplicate sequence is an optimistic lock conflict.
fn mongo_persistence_error(error: MongoError) -> PersistenceError {
    if is_duplicate_key(&error) {
        return PersistenceError::OptimisticLockError;
    }
    match error.kind.as_ref() {
        ErrorKind::BsonDeserialization(_) => PersistenceError::DeserializationError(Box::new(error)),
        _ => PersistenceError::UnknownError(Box::new(error)),
    }
}
