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
    view_repository::SoftDeletable as _,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

#[derive(Default)]
struct WorkerRegistry {
    workers: Mutex<HashMap<String, AbortHandle>>,
}

impl Drop for WorkerRegistry {
    fn drop(&mut self) {
        let mut guard = self.workers.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, handle) in guard.drain() {
            handle.abort();
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
    allow_local_network: bool,
}

impl EventPublisherSupervisor {
    #[must_use]
    pub fn new(event_bus: EventBusHandle) -> Self {
        Self {
            event_bus,
            registry: Arc::new(WorkerRegistry::default()),
            allow_local_network: cfg!(feature = "allow-localhost"),
        }
    }

    pub(crate) fn with_allow_local_network(mut self, allow: bool) -> Self {
        self.allow_local_network = allow;
        self
    }

    /// Hydrates configured publishers from the persisted read model at application startup,
    /// starting background worker tasks for all enabled, non-deleted publishers.
    pub fn hydrate_from_views(&self, views: impl IntoIterator<Item = EventPublisherView>) {
        let mut hydrated_count = 0usize;
        for view in views {
            if !view.is_deleted() && view.enabled {
                if let Some(config) = view.config {
                    self.start_or_replace_worker(view.id, &view.filter, config);
                    hydrated_count += 1;
                } else {
                    warn!(
                        publisher_id = %view.id,
                        "Skipping enabled event publisher during startup hydration: view has no configuration"
                    );
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
            .workers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Returns `true` if a background worker task is currently running for `id`.
    #[must_use]
    pub fn is_active(&self, id: &str) -> bool {
        self.registry
            .workers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(id)
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

    fn start_or_replace_worker(&self, id: String, filter: &EventPublisherFilter, config: EventPublisherConfig) {
        let mut guard = self.registry.workers.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(previous) = guard.remove(&id) {
            previous.abort();
        }

        let abort_handle = self.spawn_worker(id.clone(), filter, config);
        debug!(publisher_id = %id, "Started background event publisher worker");
        guard.insert(id, abort_handle);
    }

    fn stop_worker(&self, id: &str) {
        let mut guard = self.registry.workers.lock().unwrap_or_else(PoisonError::into_inner);

        if let Some(handle) = guard.remove(id) {
            debug!(publisher_id = %id, "Stopped background event publisher worker");
            handle.abort();
        }
    }

    pub(crate) fn deactivate_worker(&self, id: &str) {
        self.stop_worker(id);
    }

    pub(crate) fn remove_publisher(&self, id: &str) {
        self.stop_worker(id);
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
                        self.start_or_replace_worker(aggregate_id.to_string(), filter, config.clone());
                    } else {
                        self.deactivate_worker(aggregate_id);
                    }
                }
                EventPublisherEvent::EventPublisherActivated { filter, config, .. } => {
                    self.start_or_replace_worker(aggregate_id.to_string(), filter, config.clone());
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
    use shared_kernel::event_bus::CloudEvent;
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

    async fn wait_for_request_path(mock_server: &MockServer, expected_path: &str, timeout: Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(requests) = mock_server.received_requests().await {
                if requests.iter().any(|req| req.url.path() == expected_path) {
                    return;
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for request to `{expected_path}`"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn activate_event_starts_and_replaces_worker_directly_from_committed_event() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v2"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(32);
        let supervisor = EventPublisherSupervisor::new(event_bus.clone()).with_allow_local_network(true);
        assert_eq!(supervisor.active_count(), 0);

        let filter = EventPublisherFilter {
            types: vec!["test.supervisor.event".to_string()],
        };
        let activated_v1 = EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 2,
            payload: EventPublisherEvent::EventPublisherActivated {
                filter: filter.clone(),
                config: EventPublisherConfig::Http(HttpPublisherConfig {
                    target_url: format!("{}/v1", mock_server.uri()),
                    headers: None,
                }),
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };

        // Starts directly from the committed event without any prior cache or view DB lookup.
        supervisor.dispatch("pub-1", &[activated_v1]).await;
        assert_eq!(supervisor.active_count(), 1);
        assert!(supervisor.is_active("pub-1"));

        event_bus.publish(CloudEvent::new("test.supervisor.event", "/services/test"));
        wait_for_request_path(&mock_server, "/v1", Duration::from_secs(2)).await;

        // Re-activating with updated config replaces the previous worker cleanly.
        let activated_v2 = EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 4,
            payload: EventPublisherEvent::EventPublisherActivated {
                filter,
                config: EventPublisherConfig::Http(HttpPublisherConfig {
                    target_url: format!("{}/v2", mock_server.uri()),
                    headers: None,
                }),
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };
        supervisor.dispatch("pub-1", &[activated_v2]).await;
        assert_eq!(supervisor.active_count(), 1);
        assert!(supervisor.is_active("pub-1"));

        event_bus.publish(CloudEvent::new("test.supervisor.event", "/services/test"));
        wait_for_request_path(&mock_server, "/v2", Duration::from_secs(2)).await;

        let deactivated = EventEnvelope {
            aggregate_id: "pub-1".to_string(),
            sequence: 5,
            payload: EventPublisherEvent::EventPublisherDeactivated {
                occurred_at: Utc::now(),
            },
            metadata: HashMap::new(),
        };
        supervisor.dispatch("pub-1", &[deactivated]).await;
        assert_eq!(supervisor.active_count(), 0);
        assert!(!supervisor.is_active("pub-1"));

        mock_server.verify().await;
    }

    #[tokio::test]
    async fn hydrate_from_views_starts_only_enabled_non_deleted_publishers() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/active"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/inactive"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(32);
        let supervisor = EventPublisherSupervisor::new(event_bus.clone()).with_allow_local_network(true);

        supervisor.hydrate_from_views([
            make_view(
                "pub-active",
                format!("{}/active", mock_server.uri()),
                true,
                Status::Configured,
            ),
            make_view(
                "pub-paused",
                format!("{}/inactive", mock_server.uri()),
                false,
                Status::Configured,
            ),
            make_view(
                "pub-deleted",
                format!("{}/inactive", mock_server.uri()),
                true,
                Status::None,
            ),
        ]);

        assert_eq!(supervisor.active_count(), 1);
        assert!(supervisor.is_active("pub-active"));
        assert!(!supervisor.is_active("pub-paused"));
        assert!(!supervisor.is_active("pub-deleted"));

        event_bus.publish(CloudEvent::new("test.supervisor.event", "/services/test"));
        wait_for_request_path(&mock_server, "/active", Duration::from_secs(2)).await;
        mock_server.verify().await;
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn hydrate_from_views_warns_when_enabled_view_lacks_configuration() {
        let supervisor = EventPublisherSupervisor::new(EventBusHandle::new(16)).with_allow_local_network(true);
        let mut missing_config = make_view(
            "pub-no-config",
            "https://example.test/hook".to_string(),
            true,
            Status::Configured,
        );
        missing_config.config = None;

        supervisor.hydrate_from_views([missing_config]);
        assert_eq!(supervisor.active_count(), 0);
        assert!(!supervisor.is_active("pub-no-config"));
        assert!(logs_contain("WARN"));
        assert!(logs_contain("pub-no-config"));
        assert!(logs_contain("view has no configuration"));
    }

    #[tokio::test]
    async fn update_and_delete_events_replace_or_stop_workers_according_to_enabled_state() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/updated"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/disabled"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(32);
        let supervisor = EventPublisherSupervisor::new(event_bus.clone()).with_allow_local_network(true);
        let filter = EventPublisherFilter {
            types: vec!["test.supervisor.update".to_string()],
        };

        // Disabled update on inactive publisher must not start a worker.
        supervisor
            .dispatch(
                "pub-upd",
                &[EventEnvelope {
                    aggregate_id: "pub-upd".to_string(),
                    sequence: 2,
                    payload: EventPublisherEvent::EventPublisherUpdated {
                        name: "Paused".to_string(),
                        description: None,
                        enabled: false,
                        filter: filter.clone(),
                        config: EventPublisherConfig::Http(HttpPublisherConfig {
                            target_url: format!("{}/disabled", mock_server.uri()),
                            headers: None,
                        }),
                        occurred_at: Utc::now(),
                    },
                    metadata: HashMap::new(),
                }],
            )
            .await;
        assert!(!supervisor.is_active("pub-upd"));

        // Enabled update starts/replaces worker with the new config.
        supervisor
            .dispatch(
                "pub-upd",
                &[EventEnvelope {
                    aggregate_id: "pub-upd".to_string(),
                    sequence: 3,
                    payload: EventPublisherEvent::EventPublisherUpdated {
                        name: "Active".to_string(),
                        description: None,
                        enabled: true,
                        filter: filter.clone(),
                        config: EventPublisherConfig::Http(HttpPublisherConfig {
                            target_url: format!("{}/updated", mock_server.uri()),
                            headers: None,
                        }),
                        occurred_at: Utc::now(),
                    },
                    metadata: HashMap::new(),
                }],
            )
            .await;
        assert!(supervisor.is_active("pub-upd"));

        event_bus.publish(CloudEvent::new("test.supervisor.update", "/services/test"));
        wait_for_request_path(&mock_server, "/updated", Duration::from_secs(2)).await;

        // Deleted event stops the active worker.
        supervisor
            .dispatch(
                "pub-upd",
                &[EventEnvelope {
                    aggregate_id: "pub-upd".to_string(),
                    sequence: 4,
                    payload: EventPublisherEvent::EventPublisherDeleted {
                        occurred_at: Utc::now(),
                    },
                    metadata: HashMap::new(),
                }],
            )
            .await;
        assert!(!supervisor.is_active("pub-upd"));
        assert_eq!(supervisor.active_count(), 0);

        mock_server.verify().await;
    }
}
