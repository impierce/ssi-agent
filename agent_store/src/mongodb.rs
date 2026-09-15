use crate::event_verification::{self, EventVerificationError, EventVerificationReport, EventVerifier, RawStoredEvent};
use crate::{
    custom_queries::{modify_via_store, MutableViewRepository},
    in_memory::InMemoryViewRepository,
    mongodb_lease::{LeasedMongoEventRepository, WriterLease},
    replay::{ReplayError, ReplayJob, ReplayProgress, ReplayProjection, ReplayReport},
    AggregateHandler, CqrsComponentBuilder,
};
use agent_shared::{
    application_state::Command,
    config::{config, ViewStorage},
};
use async_trait::async_trait;
use cqrs_es::persist::{PersistedEventStore, PersistenceError};
use cqrs_es::CqrsFramework;
use cqrs_es::{Aggregate, Query, View};
use mongo_es::{default_mongo_client, Client, MongoEventRepository, MongoViewRepository};
use mongodb::bson::{self, doc, Document};
use mongodb::{options::FindOptions, Cursor, IndexModel};
use shared_kernel::view_repository::DynViewRepository;
use std::sync::{Arc, Mutex};

#[async_trait]
impl<V, A> MutableViewRepository<V, A> for MongoViewRepository<V, A>
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

impl<A> AggregateHandler<A, PersistedEventStore<MongoEventRepository, A>>
where
    A: Aggregate,
{
    async fn new(client: Client, services: A::Services) -> Self {
        let repo = MongoEventRepository::new(client)
            .await
            .expect("Failed to create MongoEventRepository");
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
            execution: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl<A> AggregateHandler<A, PersistedEventStore<LeasedMongoEventRepository, A>>
where
    A: Aggregate,
{
    async fn new_leased(client: Client, lease: Arc<WriterLease>, services: A::Services) -> Self {
        let repo = LeasedMongoEventRepository::new(client, lease)
            .await
            .expect("Failed to create leased MongoDB event repository");
        let store = PersistedEventStore::new_event_store(repo);
        Self {
            cqrs: CqrsFramework::new(store, vec![], services),
            execution: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

pub struct MongoDB {
    pub client: Client,
    view_storage: ViewStorage,
    replay_jobs: Mutex<Vec<Arc<dyn ReplayJob>>>,
    writer_lease: Arc<WriterLease>,
}

impl MongoDB {
    pub async fn new() -> Self {
        let connection_string = config().event_store.connection_string.clone().expect(
            "Missing config parameter `event_store.connection_string` or `UNICORE__EVENT_STORE__CONNECTION_STRING`",
        );
        let client = default_mongo_client(&connection_string).await;
        let database = client
            .default_database()
            .expect("MongoDB connection string must name a database");
        database
            .collection::<Document>("events")
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "aggregate_type": 1, "_id": 1 })
                    .build(),
            )
            .await
            .expect("Failed to create event replay index");
        let writer_lease = WriterLease::new(client.clone());
        Self {
            client,
            view_storage: config().event_store.views,
            replay_jobs: Mutex::new(Vec::new()),
            writer_lease,
        }
    }
    // TODO: Run [Client::shutdown] during graceful shutdown to close all open connections.

    pub async fn verify_events(&self) -> Result<EventVerificationReport, EventVerificationError> {
        self.verify_events_with(event_verification::core_event_verifiers())
            .await
    }

    pub fn uses_in_memory_views(&self) -> bool {
        self.view_storage == ViewStorage::InMemory
    }

    pub async fn acquire_writer_lease(&self) -> Result<(), mongodb::error::Error> {
        if self.uses_in_memory_views() {
            self.writer_lease.acquire().await?;
        }
        Ok(())
    }

    pub async fn writer_lease_lost(&self) {
        if self.uses_in_memory_views() {
            self.writer_lease.lost().await;
        } else {
            std::future::pending().await
        }
    }

    pub async fn shutdown(&self) {
        if self.uses_in_memory_views() {
            self.writer_lease.release().await;
        }
        self.client.clone().shutdown().await;
    }

    pub async fn replay_views(&self) -> Result<Vec<ReplayReport>, ReplayError> {
        let jobs = self.replay_jobs.lock().expect("replay jobs lock poisoned").clone();
        let jobs_by_type = jobs
            .iter()
            .map(|job| (job.aggregate_type(), job.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut progress = jobs
            .iter()
            .map(|job| (job.aggregate_type(), ReplayProgress::default()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut cursor = self
            .raw_event_cursor(doc! {}, doc! { "aggregate_type": 1, "_id": 1 })
            .await?;

        while cursor.advance().await.map_err(EventVerificationError::from)? {
            let document = cursor.deserialize_current().map_err(EventVerificationError::from)?;
            let raw = Self::raw_stored_event(document)?;
            let job = jobs_by_type
                .get(raw.aggregate_type.as_str())
                .ok_or_else(|| ReplayError::UnknownAggregate {
                    aggregate_type: raw.aggregate_type.clone(),
                })?;
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
        Ok(reports)
    }

    pub async fn verify_events_with(
        &self,
        verifiers: &[EventVerifier],
    ) -> Result<EventVerificationReport, EventVerificationError> {
        let mut cursor = self
            .raw_event_cursor(doc! {}, doc! { "aggregate_type": 1, "_id": 1 })
            .await?;
        let mut report = EventVerificationReport::default();

        while cursor.advance().await? {
            let document = cursor.deserialize_current()?;
            report.verify_event(Self::raw_stored_event(document)?, verifiers);
        }

        Ok(report)
    }

    pub(crate) async fn raw_event_cursor(
        &self,
        filter: Document,
        sort: Document,
    ) -> Result<Cursor<Document>, EventVerificationError> {
        let Some(database) = self.client.default_database() else {
            return Err(EventVerificationError::MissingMongoDefaultDatabase);
        };

        let collection = database.collection::<Document>("events");
        let options = FindOptions::builder().sort(sort).build();

        Ok(collection.find(filter).with_options(options).await?)
    }

    pub(crate) fn raw_stored_event(document: Document) -> Result<RawStoredEvent, EventVerificationError> {
        #[derive(serde::Deserialize)]
        struct RawStoredEventDocument {
            aggregate_type: String,
            aggregate_id: String,
            sequence: i64,
            event_type: String,
            event_version: String,
            payload: serde_json::Value,
        }

        impl From<RawStoredEventDocument> for RawStoredEvent {
            fn from(event: RawStoredEventDocument) -> Self {
                Self {
                    aggregate_type: event.aggregate_type,
                    aggregate_id: event.aggregate_id,
                    sequence: event.sequence,
                    event_type: event.event_type,
                    event_version: event.event_version,
                    payload: event.payload,
                }
            }
        }

        let event: RawStoredEventDocument = bson::from_document(document)?;
        Ok(event.into())
    }
}

impl CqrsComponentBuilder for MongoDB {
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

        match self.view_storage {
            ViewStorage::Persisted => {
                let aggregate: Arc<MongoViewRepository<V, A>> =
                    Arc::new(MongoViewRepository::new(A::TYPE, self.client.clone()));
                let all_aggregates: Arc<MongoViewRepository<AV, A>> =
                    Arc::new(MongoViewRepository::new(&all_aggregates_name, self.client.clone()));

                (
                    Arc::new(
                        AggregateHandler::new(self.client.clone(), services)
                            .await
                            .with_parameters(
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
            ViewStorage::InMemory => {
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
                        AggregateHandler::new_leased(self.client.clone(), self.writer_lease.clone(), services)
                            .await
                            .with_parameters(
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::connection::{
        aggregate::Connection,
        views::{all_connections::AllConnectionsView, ConnectionView},
    };
    #[tokio::test]
    async fn index_map_loads_legacy_bson_document_in_field_order() {
        let Ok(connection_string) = std::env::var("SSI_AGENT_TEST_MONGODB_URI") else {
            return;
        };
        let client = default_mongo_client(&connection_string).await;
        let collection = client
            .default_database()
            .unwrap()
            .collection::<Document>("all_connections_order_test");
        let view_id = uuid::Uuid::new_v4().to_string();
        let older = ConnectionView {
            connection_id: "older".to_string(),
            ..Default::default()
        };
        let newer = ConnectionView {
            connection_id: "newer".to_string(),
            ..Default::default()
        };
        collection
            .insert_one(doc! {
                "view_id": &view_id,
                "version": 1_i64,
                "payload": {
                    "older": bson::to_bson(&older).unwrap(),
                    "newer": bson::to_bson(&newer).unwrap(),
                },
            })
            .await
            .unwrap();

        let repository =
            MongoViewRepository::<AllConnectionsView, Connection>::new("all_connections_order_test", client.clone());
        let view = cqrs_es::persist::ViewRepository::load(&repository, &view_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            view.connections.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["older", "newer"]
        );
        collection.delete_one(doc! { "view_id": view_id }).await.unwrap();
        client.shutdown().await;
    }
}
