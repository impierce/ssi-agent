use crate::{
    application::views::event_publisher::EventPublisherView,
    domain::event_publisher::{
        aggregate::EventPublisher,
        config::{EventPublisherConfig, EventPublisherFilter},
        event::EventPublisherEvent,
    },
    infrastructure::{
        http::{HttpEventPublisherWorker, DEFAULT_WEBHOOK_TIMEOUT},
        nats::NatsEventPublisherWorker,
    },
};
use shared_kernel::{
    async_trait,
    cqrs_es::{EventEnvelope, Query},
    event_bus::EventBusHandle,
    view_repository::{load_by_id, BoxedViewRepository, SoftDeletable as _},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

struct PublisherEntry {
    filter: EventPublisherFilter,
    config: EventPublisherConfig,
    worker: Option<AbortHandle>,
}

#[derive(Default)]
struct WorkerRegistry {
    entries: Mutex<HashMap<String, PublisherEntry>>,
}

impl Drop for WorkerRegistry {
    fn drop(&mut self) {
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, entry) in guard.drain() {
            if let Some(handle) = entry.worker {
                handle.abort();
            }
        }
    }
}

/// Supervises active background event publisher workers (`HTTP`, `NATS`, etc.),
/// starting, replacing, or stopping Tokio tasks immediately as [`EventPublisherEvent`]s
/// are committed, and hydrating configured publishers from the read model on startup.
///
/// Runtime event publisher supervision is single-replica for v0: workers run in-process on the
/// node handling commands and events rather than synchronizing across replicas (which would cause
/// duplicate webhook deliveries when every replica observes the same change stream).
#[derive(Clone)]
pub struct EventPublisherSupervisor {
    event_bus: EventBusHandle,
    registry: Arc<WorkerRegistry>,
    view_repo: Option<Arc<BoxedViewRepository<EventPublisherView, EventPublisher>>>,
    allow_local_network: bool,
}

impl EventPublisherSupervisor {
    #[must_use]
    pub fn new(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            registry: Arc::new(WorkerRegistry::default()),
            view_repo: None,
            allow_local_network: cfg!(feature = "allow-localhost"),
        }
    }

    pub(crate) fn with_allow_local_network(mut self, allow: bool) -> Self {
        self.allow_local_network = allow;
        self
    }

    /// Attaches the [`EventPublisherView`] repository so activating a publisher loads the latest
    /// persisted view instead of relying on potentially stale in-memory state.
    #[must_use]
    pub fn with_view_repository(
        mut self,
        view_repo: Arc<BoxedViewRepository<EventPublisherView, EventPublisher>>,
    ) -> Self {
        self.view_repo = Some(view_repo);
        self
    }

    /// Hydrates configured publishers from the persisted read model at application startup,
    /// starting background worker tasks for all enabled, non-deleted publishers and retaining
    /// inactive configurations for subsequent activation.
    pub fn hydrate_from_views(&self, views: impl IntoIterator<Item = EventPublisherView>) {
        let mut hydrated_count = 0usize;
        for view in views {
            if !view.is_deleted() {
                if let Some(config) = view.config {
                    if view.enabled {
                        self.start_or_replace_worker(view.id, view.filter, config);
                        hydrated_count += 1;
                    } else {
                        self.upsert_inactive(view.id, view.filter, config);
                    }
                }
            }
        }
        if hydrated_count > 0 {
            info!(hydrated_count, "Hydrated active event publishers from read model");
        }
    }

    /// Returns the number of currently active publisher worker tasks.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.registry
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|entry| entry.worker.is_some())
            .count()
    }

    /// Returns `true` if a background worker task is currently running for `id`.
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        self.registry
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .is_some_and(|entry| entry.worker.is_some())
    }

    fn spawn_worker(&self, id: String, filter: &EventPublisherFilter, config: EventPublisherConfig) -> AbortHandle {
        match config {
            EventPublisherConfig::Http(http_config) => HttpEventPublisherWorker::with_options(
                id,
                filter,
                http_config,
                &self.event_bus,
                DEFAULT_WEBHOOK_TIMEOUT,
                self.allow_local_network,
            )
            .spawn()
            .abort_handle(),
            EventPublisherConfig::Nats(nats_config) => {
                NatsEventPublisherWorker::spawn_resilient(id, filter, nats_config, &self.event_bus).abort_handle()
            }
        }
    }

    fn start_or_replace_worker(&self, id: String, filter: EventPublisherFilter, config: EventPublisherConfig) {
        let mut guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(handle) = guard.remove(&id).and_then(|previous| previous.worker) {
            handle.abort();
        }

        let abort_handle = self.spawn_worker(id.clone(), &filter, config.clone());
        debug!(publisher_id = %id, "Started background event publisher worker");
        guard.insert(
            id,
            PublisherEntry {
                filter,
                config,
                worker: Some(abort_handle),
            },
        );
    }

    fn upsert_inactive(&self, id: String, filter: EventPublisherFilter, config: EventPublisherConfig) {
        let mut guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(handle) = guard.remove(&id).and_then(|previous| previous.worker) {
            debug!(publisher_id = %id, "Stopped background event publisher worker");
            handle.abort();
        }

        guard.insert(
            id,
            PublisherEntry {
                filter,
                config,
                worker: None,
            },
        );
    }

    async fn activate_worker(&self, id: &str) {
        if let Some(view_repo) = &self.view_repo {
            match load_by_id(view_repo.as_ref(), id).await {
                Ok(Some(view)) => {
                    if let Some(config) = view.config {
                        self.start_or_replace_worker(id.to_string(), view.filter, config);
                    } else {
                        self.remove_publisher(id);
                        warn!(
                            publisher_id = %id,
                            "Cannot start background event publisher worker: persisted view has no configuration"
                        );
                    }
                }
                Ok(None) => {
                    self.remove_publisher(id);
                    warn!(
                        publisher_id = %id,
                        "Cannot start background event publisher worker: persisted view not found or deleted"
                    );
                }
                Err(err) => {
                    warn!(
                        publisher_id = %id,
                        error = %err,
                        "Failed to load event publisher view during worker activation"
                    );
                }
            }
            return;
        }

        let cached = {
            let guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);
            guard.get(id).map(|entry| (entry.filter.clone(), entry.config.clone()))
        };

        if let Some((filter, config)) = cached {
            self.start_or_replace_worker(id.to_string(), filter, config);
            return;
        }

        warn!(
            publisher_id = %id,
            "Cannot start background event publisher worker: configuration not found"
        );
    }

    fn deactivate_worker(&self, id: &str) {
        let mut guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(handle) = guard.get_mut(id).and_then(|entry| entry.worker.take()) {
            debug!(publisher_id = %id, "Stopped background event publisher worker");
            handle.abort();
        }
    }

    fn remove_publisher(&self, id: &str) {
        let mut guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(handle) = guard.remove(id).and_then(|entry| entry.worker) {
            debug!(publisher_id = %id, "Stopped background event publisher worker");
            handle.abort();
        }
    }
}

#[async_trait]
impl Query<EventPublisher> for EventPublisherSupervisor {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<EventPublisher>]) {
        for event in events {
            match &event.payload {
                EventPublisherEvent::EventPublisherCreated {
                    enabled,
                    filter,
                    config,
                    ..
                }
                | EventPublisherEvent::EventPublisherUpdated {
                    enabled,
                    filter,
                    config,
                    ..
                } => {
                    if *enabled {
                        self.start_or_replace_worker(aggregate_id.to_string(), filter.clone(), config.clone());
                    } else {
                        self.upsert_inactive(aggregate_id.to_string(), filter.clone(), config.clone());
                    }
                }
                EventPublisherEvent::EventPublisherActivated { .. } => {
                    self.activate_worker(aggregate_id).await;
                }
                EventPublisherEvent::EventPublisherDeactivated { .. } => {
                    self.deactivate_worker(aggregate_id);
                }
                EventPublisherEvent::EventPublisherDeleted { .. } => {
                    self.remove_publisher(aggregate_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::views::event_publisher::Status, domain::event_publisher::config::HttpPublisherConfig};
    use chrono::Utc;
    use shared_kernel::{
        cqrs_es::persist::{ViewContext, ViewRepository as _},
        event_bus::CloudEvent,
        test_utils::in_memory::MemViewRepository,
        view_repository::BoxedViewRepository,
    };
    use std::time::Duration;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn make_view(id: &str, target_url: String, enabled: bool, status: Status) -> EventPublisherView {
        let now = Utc::now();
        EventPublisherView {
            id: id.to_string(),
            name: "Test".to_string(),
            description: None,
            enabled,
            filter: EventPublisherFilter {
                types: vec!["test.supervisor.event".to_string()],
            },
            config: (status == Status::Configured).then_some(EventPublisherConfig::Http(HttpPublisherConfig {
                target_url,
                headers: None,
            })),
            view_status: status,
            created_at: now,
            last_modified_at: now,
        }
    }

    #[tokio::test]
    async fn activate_worker_prefers_persisted_view_over_stale_cache_and_never_resurrects_missing_or_deleted() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/stale"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/fresh"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(32);
        let view_repo = Arc::new(BoxedViewRepository::new(Box::new(MemViewRepository::<
            EventPublisherView,
            EventPublisher,
        >::default())));

        // Persist fresh view pointing to `/fresh`
        view_repo
            .update_view(
                make_view(
                    "pub-1",
                    format!("{}/fresh", mock_server.uri()),
                    true,
                    Status::Configured,
                ),
                ViewContext::new("pub-1".to_string(), 0),
            )
            .await
            .unwrap();

        let supervisor = EventPublisherSupervisor::new(event_bus.clone())
            .with_view_repository(view_repo.clone())
            .with_allow_local_network(true);

        // Seed in-memory cache with stale `/stale` config (inactive)
        supervisor.hydrate_from_views([make_view(
            "pub-1",
            format!("{}/stale", mock_server.uri()),
            false,
            Status::Configured,
        )]);
        assert!(!supervisor.is_active("pub-1"));

        let activated = EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 2,
            payload: EventPublisherEvent::EventPublisherActivated {
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };

        // 1. Activating loads `/fresh` from view_repo instead of stale cached `/stale`
        supervisor.dispatch("pub-1", std::slice::from_ref(&activated)).await;
        assert!(supervisor.is_active("pub-1"));

        event_bus.publish(CloudEvent::new("test.supervisor.event", "/services/test"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        mock_server.verify().await;

        // 2. If persisted view is soft-deleted, activate_worker must NOT fall back to stale cache
        view_repo
            .update_view(
                make_view("pub-1", format!("{}/stale", mock_server.uri()), false, Status::None),
                ViewContext::new("pub-1".to_string(), 1),
            )
            .await
            .unwrap();
        supervisor.dispatch("pub-1", std::slice::from_ref(&activated)).await;
        assert!(!supervisor.is_active("pub-1"));

        // 3. If persisted view is missing (Ok(None)), activate_worker must NOT fall back to stale cache
        supervisor.hydrate_from_views([make_view(
            "pub-missing",
            format!("{}/stale", mock_server.uri()),
            false,
            Status::Configured,
        )]);
        let missing_activated = EventEnvelope {
            aggregate_id: "pub-missing".to_string(),
            sequence: 2,
            payload: EventPublisherEvent::EventPublisherActivated {
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };
        supervisor.dispatch("pub-missing", &[missing_activated]).await;
        assert!(!supervisor.is_active("pub-missing"));
    }

    #[tokio::test]
    async fn activate_worker_falls_back_to_cache_when_no_view_repository_is_attached() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/cached"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(32);
        let supervisor = EventPublisherSupervisor::new(event_bus.clone()).with_allow_local_network(true);
        supervisor.hydrate_from_views([make_view(
            "pub-cached",
            format!("{}/cached", mock_server.uri()),
            false,
            Status::Configured,
        )]);

        let activated = EventEnvelope {
            aggregate_id: "pub-cached".to_string(),
            sequence: 2,
            payload: EventPublisherEvent::EventPublisherActivated {
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };
        supervisor.dispatch("pub-cached", &[activated]).await;
        assert!(supervisor.is_active("pub-cached"));

        event_bus.publish(CloudEvent::new("test.supervisor.event", "/services/test"));
        tokio::time::sleep(Duration::from_millis(150)).await;
        mock_server.verify().await;
    }
}
