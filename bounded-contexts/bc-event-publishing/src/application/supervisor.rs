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
    view_repository::SoftDeletable as _,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::task::AbortHandle;
use tracing::info;

#[derive(Default)]
struct WorkerRegistry {
    workers: Mutex<HashMap<String, AbortHandle>>,
}

impl Drop for WorkerRegistry {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.workers.lock() {
            for (_, handle) in guard.drain() {
                handle.abort();
            }
        }
    }
}

/// Supervises active background event publisher workers (`HTTP`, `NATS`, etc.),
/// starting, replacing, or stopping Tokio tasks immediately as [`EventPublisherEvent`]s
/// are committed, and hydrating active workers from the read model on startup.
#[derive(Clone)]
pub struct EventPublisherSupervisor {
    event_bus: EventBusHandle,
    registry: Arc<WorkerRegistry>,
}

impl EventPublisherSupervisor {
    #[must_use]
    pub fn new(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            registry: Arc::new(WorkerRegistry::default()),
        }
    }

    /// Hydrates and starts background worker tasks for all enabled, non-deleted publishers
    /// from the persisted read model at application startup.
    pub fn hydrate_from_views(&self, views: impl IntoIterator<Item = EventPublisherView>) {
        let mut hydrated_count = 0usize;
        for view in views {
            if !view.is_deleted() && view.enabled {
                if let Some(config) = view.config {
                    self.start_or_replace_worker(view.id, view.filter, config);
                    hydrated_count += 1;
                }
            }
        }
        info!(hydrated_count, "Hydrated active event publishers from read model");
    }

    /// Returns the number of currently active publisher worker tasks.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.registry.workers.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Returns `true` if a background worker task is currently running for `id`.
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        self.registry
            .workers
            .lock()
            .map(|g| g.contains_key(id))
            .unwrap_or(false)
    }

    fn start_or_replace_worker(&self, id: String, filter: EventPublisherFilter, config: EventPublisherConfig) {
        let Ok(mut guard) = self.registry.workers.lock() else {
            return;
        };

        if let Some(previous) = guard.remove(&id) {
            info!(publisher_id = %id, "Stopping previous event publisher worker before replacement");
            previous.abort();
        }

        let abort_handle = match config {
            EventPublisherConfig::Http(http_config) => {
                HttpEventPublisherWorker::new(id.clone(), filter, http_config, &self.event_bus)
                    .spawn()
                    .abort_handle()
            }
            EventPublisherConfig::Nats(nats_config) => {
                NatsEventPublisherWorker::spawn_resilient(id.clone(), filter, nats_config, &self.event_bus)
                    .abort_handle()
            }
        };

        info!(publisher_id = %id, "Started background event publisher worker");
        guard.insert(id, abort_handle);
    }

    fn stop_worker(&self, id: &str) {
        let Ok(mut guard) = self.registry.workers.lock() else {
            return;
        };

        if let Some(handle) = guard.remove(id) {
            info!(publisher_id = %id, "Stopped background event publisher worker");
            handle.abort();
        }
    }
}

#[async_trait]
impl Query<EventPublisher> for EventPublisherSupervisor {
    async fn dispatch(&self, _aggregate_id: &str, events: &[EventEnvelope<EventPublisher>]) {
        for event in events {
            match &event.payload {
                EventPublisherEvent::EventPublisherCreated {
                    id,
                    enabled,
                    filter,
                    config,
                    ..
                }
                | EventPublisherEvent::EventPublisherUpdated {
                    id,
                    enabled,
                    filter,
                    config,
                    ..
                } => {
                    if *enabled {
                        self.start_or_replace_worker(id.clone(), filter.clone(), config.clone());
                    } else {
                        self.stop_worker(id);
                    }
                }
                EventPublisherEvent::EventPublisherActivated { id, filter, config, .. } => {
                    self.start_or_replace_worker(id.clone(), filter.clone(), config.clone());
                }
                EventPublisherEvent::EventPublisherDeactivated { id, .. }
                | EventPublisherEvent::EventPublisherDeleted { id, .. } => {
                    self.stop_worker(id);
                }
            }
        }
    }
}
