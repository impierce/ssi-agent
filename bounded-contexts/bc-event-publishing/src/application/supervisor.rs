use crate::{
    application::views::event_publisher::EventPublisherView,
    domain::event_publisher::{
        aggregate::EventPublisher,
        config::{EventPublisherConfig, EventPublisherFilter},
        event::EventPublisherEvent,
    },
    infrastructure::{http::HttpEventPublisherWorker, nats::NatsEventPublisherWorker},
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
#[derive(Clone)]
pub struct EventPublisherSupervisor {
    event_bus: EventBusHandle,
    registry: Arc<WorkerRegistry>,
    view_repo: Option<Arc<BoxedViewRepository<EventPublisherView, EventPublisher>>>,
}

impl EventPublisherSupervisor {
    #[must_use]
    pub fn new(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            registry: Arc::new(WorkerRegistry::default()),
            view_repo: None,
        }
    }

    /// Attaches the [`EventPublisherView`] repository so activating a publisher whose configuration
    /// was created on another replica after startup can fall back to loading the persisted view.
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
            EventPublisherConfig::Http(http_config) => {
                HttpEventPublisherWorker::new(id, filter, http_config, &self.event_bus)
                    .spawn()
                    .abort_handle()
            }
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
        let cached = {
            let guard = self.registry.entries.lock().unwrap_or_else(PoisonError::into_inner);
            guard.get(id).map(|entry| (entry.filter.clone(), entry.config.clone()))
        };

        if let Some((filter, config)) = cached {
            self.start_or_replace_worker(id.to_string(), filter, config);
            return;
        }

        if let Some(view_repo) = &self.view_repo {
            match load_by_id(view_repo.as_ref(), id).await {
                Ok(Some(view)) => {
                    if let Some(config) = view.config {
                        self.start_or_replace_worker(id.to_string(), view.filter, config);
                        return;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    warn!(
                        publisher_id = %id,
                        error = %err,
                        "Failed to load event publisher view during worker activation"
                    );
                    return;
                }
            }
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
