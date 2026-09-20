use cqrs_es::{
    persist::{PersistedEventRepository, PersistenceError, ReplayStream, SerializedEvent, SerializedSnapshot},
    Aggregate,
};
use mongodb::{
    bson::{self, doc, DateTime, Document},
    error::{Error as MongoError, ErrorKind, WriteFailure},
    options::{IndexOptions, ReturnDocument},
    Client, ClientSession, Collection, Cursor, IndexModel,
};
use serde_json::Value;
use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{error, info, warn};
use uuid::Uuid;

const EVENT_COLLECTION: &str = "events";
const SNAPSHOT_COLLECTION: &str = "snapshots";
const LEASE_COLLECTION: &str = "application_leases";
const LEASE_ID: &str = "in_memory_projection_writer";
const STREAM_CHANNEL_SIZE: usize = 200;

#[derive(Clone, Copy)]
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

#[derive(Debug)]
struct LeaseLost;

impl fmt::Display for LeaseLost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MongoDB in-memory projection writer lease is not active")
    }
}

impl Error for LeaseLost {}

pub(crate) struct WriterLease {
    client: Client,
    owner_id: String,
    token: AtomicI64,
    active: AtomicBool,
    options: WriterLeaseOptions,
    stop: watch::Sender<bool>,
    lost: watch::Sender<bool>,
    renewal_task: Mutex<Option<JoinHandle<()>>>,
}

impl WriterLease {
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
            .ok_or_else(|| MongoError::custom("MongoDB client has no default database configured"))
    }

    pub(crate) async fn acquire(self: &Arc<Self>) -> Result<(), MongoError> {
        let collection = self.collection()?;
        collection
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
            .await?;

        loop {
            let lease = collection
                .find_one_and_update(
                    doc! {
                        "_id": LEASE_ID,
                        "$expr": { "$lte": ["$expires_at", "$$NOW"] },
                    },
                    vec![doc! {
                        "$set": {
                            "owner_id": &self.owner_id,
                            "fencing_token": {
                                "$add": [{ "$ifNull": ["$fencing_token", 0_i64] }, 1_i64]
                            },
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
                self.active.store(true, Ordering::Release);
                self.lost.send_replace(false);
                info!(owner_id = %self.owner_id, fencing_token = token, "Acquired MongoDB writer lease");
                self.start_renewal();
                return Ok(());
            }

            tokio::time::sleep(self.options.retry_interval).await;
        }
    }

    fn start_renewal(self: &Arc<Self>) {
        let lease = Arc::downgrade(self);
        let options = self.options;
        let mut stop = self.stop.subscribe();
        let task = tokio::spawn(async move {
            let mut last_renewal = Instant::now();
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
                        match lease.renew().await {
                            Ok(true) => last_renewal = Instant::now(),
                            Ok(false) => {
                                lease.mark_lost();
                                return;
                            }
                            Err(renewal_error) if last_renewal.elapsed() < options.ttl => {
                                warn!(error = %renewal_error, "MongoDB writer lease renewal failed; retrying before expiry");
                            }
                            Err(renewal_error) => {
                                error!(error = %renewal_error, "MongoDB writer lease could not be renewed before expiry");
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
        let task = self.renewal_task.lock().expect("lease task lock poisoned").take();
        if let Some(task) = task {
            let _ = task.await;
        }

        if self.active.swap(false, Ordering::AcqRel) {
            match self.collection() {
                Ok(collection) => match collection
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
                                "released_at": "$$NOW",
                            }
                        }],
                    )
                    .await
                {
                    Ok(result) if result.matched_count == 1 => {
                        info!(owner_id = %self.owner_id, "Released MongoDB writer lease")
                    }
                    Ok(_) => {
                        warn!(owner_id = %self.owner_id, "MongoDB writer lease was no longer owned during release")
                    }
                    Err(release_error) => warn!(error = %release_error, "Failed to release MongoDB writer lease"),
                },
                Err(release_error) => warn!(error = %release_error, "Failed to release MongoDB writer lease"),
            }
        }
    }

    async fn fence(&self, session: &mut ClientSession) -> Result<(), PersistenceError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(PersistenceError::UnknownError(Box::new(LeaseLost)));
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
            self.mark_lost();
            return Err(PersistenceError::UnknownError(Box::new(LeaseLost)));
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

pub(crate) struct LeasedMongoEventRepository {
    client: Client,
    lease: Arc<WriterLease>,
}

impl LeasedMongoEventRepository {
    pub(crate) async fn new(client: Client, lease: Arc<WriterLease>) -> Result<Self, MongoError> {
        let repository = Self { client, lease };
        repository.create_indexes().await?;
        Ok(repository)
    }

    fn collection(&self, name: &str) -> Collection<Document> {
        self.client
            .default_database()
            .expect("MongoDB connection string must name a database")
            .collection(name)
    }

    async fn create_indexes(&self) -> Result<(), MongoError> {
        self.collection(EVENT_COLLECTION)
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "aggregate_id": 1, "sequence": 1 })
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;
        self.collection(SNAPSHOT_COLLECTION)
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "aggregate_id": 1, "current_snapshot": -1 })
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await?;
        Ok(())
    }

    async fn query_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
        min_sequence: usize,
    ) -> Result<Vec<SerializedEvent>, PersistenceError> {
        let mut cursor = self
            .collection(EVENT_COLLECTION)
            .find(event_filter::<A>(aggregate_id, min_sequence))
            .sort(doc! { "sequence": 1 })
            .await
            .map_err(mongo_persistence_error)?;
        let mut events = Vec::new();
        while cursor.advance().await.map_err(mongo_persistence_error)? {
            let document = cursor.deserialize_current().map_err(mongo_persistence_error)?;
            events.push(serialized_event(&document)?);
        }
        Ok(events)
    }

    async fn event_cursor<A: Aggregate>(
        &self,
        aggregate_id: Option<&str>,
    ) -> Result<Cursor<Document>, PersistenceError> {
        let filter = aggregate_id.map_or_else(
            || doc! { "aggregate_type": A::TYPE },
            |aggregate_id| event_filter::<A>(aggregate_id, 0),
        );
        self.collection(EVENT_COLLECTION)
            .find(filter)
            .sort(doc! { "sequence": 1 })
            .await
            .map_err(mongo_persistence_error)
    }

    async fn persist_transaction<A: Aggregate>(
        &self,
        events: &[SerializedEvent],
        snapshot_update: Option<(String, Value, usize)>,
    ) -> Result<(), PersistenceError> {
        let mut session = self.client.start_session().await.map_err(mongo_persistence_error)?;
        session.start_transaction().await.map_err(mongo_persistence_error)?;

        let result = async {
            self.lease.fence(&mut session).await?;
            if !events.is_empty() {
                self.collection(EVENT_COLLECTION)
                    .insert_many(event_documents(events)?)
                    .session(&mut session)
                    .await
                    .map_err(mongo_persistence_error)?;
            }

            if let Some((aggregate_id, aggregate_payload, current_snapshot)) = snapshot_update {
                let expected_snapshot = current_snapshot.saturating_sub(1);
                let current_sequence = events.last().map_or(0, |event| event.sequence);
                let snapshot = doc! {
                    "aggregate_type": A::TYPE,
                    "aggregate_id": &aggregate_id,
                    "payload": bson::to_bson(&aggregate_payload).map_err(bson_persistence_error)?,
                    "current_sequence": i64::try_from(current_sequence).map_err(conversion_persistence_error)?,
                    "current_snapshot": i64::try_from(current_snapshot).map_err(conversion_persistence_error)?,
                };
                let replacement = self
                    .collection(SNAPSHOT_COLLECTION)
                    .replace_one(
                        doc! {
                            "aggregate_id": &aggregate_id,
                            "aggregate_type": A::TYPE,
                            "current_snapshot": i64::try_from(expected_snapshot).map_err(conversion_persistence_error)?,
                        },
                        snapshot,
                    )
                    .upsert(true)
                    .session(&mut session)
                    .await
                    .map_err(mongo_persistence_error)?;
                if replacement.matched_count == 0 && replacement.upserted_id.is_none() {
                    return Err(PersistenceError::OptimisticLockError);
                }
            }

            session.commit_transaction().await.map_err(mongo_persistence_error)
        }
        .await;

        if result.is_err() {
            let _ = session.abort_transaction().await;
        }
        result
    }
}

impl PersistedEventRepository for LeasedMongoEventRepository {
    async fn get_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<Vec<SerializedEvent>, PersistenceError> {
        self.query_events::<A>(aggregate_id, 0).await
    }

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &str,
        last_sequence: usize,
    ) -> Result<Vec<SerializedEvent>, PersistenceError> {
        self.query_events::<A>(aggregate_id, last_sequence).await
    }

    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &str,
    ) -> Result<Option<SerializedSnapshot>, PersistenceError> {
        let snapshot = self
            .collection(SNAPSHOT_COLLECTION)
            .find_one(doc! { "aggregate_type": A::TYPE, "aggregate_id": aggregate_id })
            .sort(doc! { "current_snapshot": -1 })
            .await
            .map_err(mongo_persistence_error)?;
        snapshot
            .map(|document| serialized_snapshot(aggregate_id, &document))
            .transpose()
    }

    async fn persist<A: Aggregate>(
        &self,
        events: &[SerializedEvent],
        snapshot_update: Option<(String, Value, usize)>,
    ) -> Result<(), PersistenceError> {
        self.persist_transaction::<A>(events, snapshot_update).await
    }

    async fn stream_events<A: Aggregate>(&self, aggregate_id: &str) -> Result<ReplayStream, PersistenceError> {
        Ok(stream_events(self.event_cursor::<A>(Some(aggregate_id)).await?))
    }

    async fn stream_all_events<A: Aggregate>(&self) -> Result<ReplayStream, PersistenceError> {
        Ok(stream_events(self.event_cursor::<A>(None).await?))
    }
}

fn event_filter<A: Aggregate>(aggregate_id: &str, min_sequence: usize) -> Document {
    let mut filter = doc! { "aggregate_type": A::TYPE, "aggregate_id": aggregate_id };
    if min_sequence > 0 {
        filter.insert(
            "sequence",
            doc! { "$gte": i64::try_from(min_sequence).unwrap_or(i64::MAX) },
        );
    }
    filter
}

fn event_documents(events: &[SerializedEvent]) -> Result<Vec<Document>, PersistenceError> {
    events
        .iter()
        .map(|event| {
            Ok(doc! {
                "aggregate_id": &event.aggregate_id,
                "aggregate_type": &event.aggregate_type,
                "sequence": i64::try_from(event.sequence).map_err(conversion_persistence_error)?,
                "event_type": &event.event_type,
                "event_version": &event.event_version,
                "payload": bson::to_bson(&event.payload).map_err(bson_persistence_error)?,
                "metadata": bson::to_bson(&event.metadata).map_err(bson_persistence_error)?,
            })
        })
        .collect()
}

fn serialized_event(document: &Document) -> Result<SerializedEvent, PersistenceError> {
    Ok(SerializedEvent {
        aggregate_id: document
            .get_str("aggregate_id")
            .map_err(bson_persistence_error)?
            .to_string(),
        sequence: usize::try_from(document.get_i64("sequence").map_err(bson_persistence_error)?)
            .map_err(conversion_persistence_error)?,
        aggregate_type: document
            .get_str("aggregate_type")
            .map_err(bson_persistence_error)?
            .to_string(),
        event_type: document
            .get_str("event_type")
            .map_err(bson_persistence_error)?
            .to_string(),
        event_version: document
            .get_str("event_version")
            .map_err(bson_persistence_error)?
            .to_string(),
        payload: bson::from_bson(document.get("payload").cloned().unwrap_or_default())
            .map_err(bson_persistence_error)?,
        metadata: bson::from_bson(document.get("metadata").cloned().unwrap_or_default())
            .map_err(bson_persistence_error)?,
    })
}

fn serialized_snapshot(aggregate_id: &str, document: &Document) -> Result<SerializedSnapshot, PersistenceError> {
    Ok(SerializedSnapshot {
        aggregate_id: aggregate_id.to_string(),
        aggregate: bson::from_bson(document.get("payload").cloned().unwrap_or_default())
            .map_err(bson_persistence_error)?,
        current_sequence: usize::try_from(document.get_i64("current_sequence").map_err(bson_persistence_error)?)
            .map_err(conversion_persistence_error)?,
        current_snapshot: usize::try_from(document.get_i64("current_snapshot").map_err(bson_persistence_error)?)
            .map_err(conversion_persistence_error)?,
    })
}

fn stream_events(mut cursor: Cursor<Document>) -> ReplayStream {
    let (mut feed, stream) = ReplayStream::new(STREAM_CHANNEL_SIZE);
    tokio::spawn(async move {
        loop {
            match cursor.advance().await {
                Ok(true) => {
                    let event = cursor
                        .deserialize_current()
                        .map_err(mongo_persistence_error)
                        .and_then(|document| serialized_event(&document));
                    if feed.push(event).await.is_err() {
                        return;
                    }
                }
                Ok(false) => return,
                Err(cursor_error) => {
                    let _ = feed.push(Err(mongo_persistence_error(cursor_error))).await;
                    return;
                }
            }
        }
    });
    stream
}

fn mongo_persistence_error(error: MongoError) -> PersistenceError {
    match error.kind.as_ref() {
        ErrorKind::BsonDeserialization(_) => PersistenceError::DeserializationError(Box::new(error)),
        ErrorKind::InsertMany(failure) if failure.write_errors.iter().flatten().any(|write| write.code == 11000) => {
            PersistenceError::OptimisticLockError
        }
        ErrorKind::Write(WriteFailure::WriteError(write)) if write.code == 11000 => {
            PersistenceError::OptimisticLockError
        }
        _ => PersistenceError::UnknownError(Box::new(error)),
    }
}

fn bson_persistence_error(error: impl Error + Send + Sync + 'static) -> PersistenceError {
    PersistenceError::DeserializationError(Box::new(error))
}

fn conversion_persistence_error(error: impl Error + Send + Sync + 'static) -> PersistenceError {
    PersistenceError::UnknownError(Box::new(error))
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
        const TYPE: &'static str = "lease_test";
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

    async fn test_client() -> Option<Client> {
        let Ok(uri) = std::env::var("SSI_AGENT_TEST_MONGODB_URI") else {
            eprintln!(
                "SKIPPED {}: set SSI_AGENT_TEST_MONGODB_URI to a replica-set MongoDB to exercise the writer lease",
                std::thread::current().name().unwrap_or("test")
            );
            return None;
        };
        Some(Client::with_uri_str(uri).await.unwrap())
    }

    /// Removes only what these tests write, so they run as a least-privilege application user
    /// against a shared database. Dropping the database would need `dropDatabase` and would
    /// destroy unrelated collections.
    async fn reset_test_state(client: &Client) {
        let database = client.default_database().unwrap();
        database
            .collection::<Document>(LEASE_COLLECTION)
            .delete_many(doc! { "_id": LEASE_ID })
            .await
            .unwrap();
        database
            .collection::<Document>(EVENT_COLLECTION)
            .delete_many(doc! { "aggregate_type": TestAggregate::TYPE })
            .await
            .unwrap();
    }

    /// Acquires with a deadline: without one, a database that already has a live writer makes the
    /// test hang forever instead of reporting why.
    async fn acquire_or_fail(lease: &Arc<WriterLease>) {
        tokio::time::timeout(Duration::from_secs(10), lease.acquire())
            .await
            .expect("timed out acquiring the writer lease: another writer holds it, so point these tests at an idle database")
            .unwrap();
    }

    fn test_options() -> WriterLeaseOptions {
        WriterLeaseOptions {
            ttl: Duration::from_secs(2),
            renew_interval: Duration::from_millis(200),
            retry_interval: Duration::from_millis(20),
        }
    }

    #[tokio::test]
    async fn only_one_writer_holds_the_lease() {
        let Some(client) = test_client().await else {
            return;
        };
        reset_test_state(&client).await;
        let first = WriterLease::with_options(client.clone(), test_options());
        let second = WriterLease::with_options(client.clone(), test_options());

        acquire_or_fail(&first).await;
        let first_token = first.token.load(Ordering::Acquire);
        let waiting_lease = second.clone();
        let waiter = tokio::spawn(async move { waiting_lease.acquire().await });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiter.is_finished());

        first.release().await;
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second.token.load(Ordering::Acquire), first_token + 1);
        second.release().await;
        reset_test_state(&client).await;
    }

    #[tokio::test]
    async fn stale_writer_cannot_append_after_its_fencing_token_is_replaced() {
        let Some(client) = test_client().await else {
            return;
        };
        reset_test_state(&client).await;
        let lease = WriterLease::with_options(client.clone(), test_options());
        acquire_or_fail(&lease).await;
        let repository = LeasedMongoEventRepository::new(client.clone(), lease.clone())
            .await
            .unwrap();

        client
            .default_database()
            .unwrap()
            .collection::<Document>(LEASE_COLLECTION)
            .update_one(
                doc! { "_id": LEASE_ID },
                doc! {
                    "$set": {
                        "owner_id": "replacement",
                        "fencing_token": lease.token.load(Ordering::Acquire) + 1,
                    }
                },
            )
            .await
            .unwrap();

        let result = repository
            .persist::<TestAggregate>(
                &[SerializedEvent {
                    aggregate_id: "one".to_string(),
                    sequence: 1,
                    aggregate_type: TestAggregate::TYPE.to_string(),
                    event_type: "test".to_string(),
                    event_version: "1".to_string(),
                    payload: serde_json::json!(null),
                    metadata: Default::default(),
                }],
                None,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(
            client
                .default_database()
                .unwrap()
                .collection::<Document>(EVENT_COLLECTION)
                .count_documents(doc! { "aggregate_type": TestAggregate::TYPE })
                .await
                .unwrap(),
            0
        );
        lease.release().await;
        reset_test_state(&client).await;
    }
}
