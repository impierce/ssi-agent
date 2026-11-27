use crate::{
    application::{
        service::EventPublishingContext,
        supervisor::EventPublisherSupervisor,
        views::{event_publisher_list::EVENT_PUBLISHER_LIST_VIEW_ID, EventPublisherViewRepositories},
    },
    domain::event_publisher::{
        aggregate::EventPublisher,
        command::EventPublisherCommand,
        error::EventPublisherError,
        services::{EventPublisherDomainServices, KnownEventTypes},
    },
};
use shared_kernel::{
    async_trait,
    command_handler::{CommandExecutor, CommandHandler, CommandHandlerFactory},
    cqrs_es::{
        persist::{GenericQuery, PersistenceError},
        AggregateError, Query,
    },
    custom_queries::ListAllQuery,
    event_bus::EventBusHandle,
    view_repository::SoftDeletable as _,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Keeps the aggregate load, event write, view projections, and [`EventPublisherSupervisor`]
/// worker transitions serialized within one process so concurrent commands cannot dispatch
/// out of order to `ListAllQuery` or `EventPublisherSupervisor`.
///
/// Execution runs inside a spawned task holding the owned mutex guard so caller/HTTP
/// cancellation cannot abort post-commit view updates or supervisor transitions.
struct SerializedEventPublisherHandler {
    inner: CommandHandler<EventPublisher>,
    execution: Arc<Mutex<()>>,
    supervisor: EventPublisherSupervisor,
}

#[async_trait]
impl CommandExecutor<EventPublisher> for SerializedEventPublisherHandler {
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: EventPublisherCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<EventPublisherError>> {
        let guard = Arc::clone(&self.execution).lock_owned().await;
        let inner = Arc::clone(&self.inner);
        let supervisor = self.supervisor.clone();
        let aggregate_id = aggregate_id.to_string();

        let is_deactivate = matches!(command, EventPublisherCommand::DeactivateEventPublisher);
        let is_delete = matches!(command, EventPublisherCommand::DeleteEventPublisher);

        tokio::spawn(async move {
            let _guard = guard;
            let result = inner.execute_with_metadata(&aggregate_id, command, metadata).await;
            if is_deactivate && result.is_ok() {
                supervisor.deactivate_worker(&aggregate_id);
            } else if is_delete
                && matches!(
                    result,
                    Ok(()) | Err(AggregateError::UserError(EventPublisherError::EventPublisherNotFound))
                )
            {
                supervisor.remove_publisher(&aggregate_id);
            }
            result
        })
        .await
        .map_err(|err| AggregateError::UnexpectedError(Box::new(err)))?
    }
}

pub struct EventPublishingContextBuilder {
    views: Option<EventPublisherViewRepositories>,
    event_bus: Option<EventBusHandle>,
    known_event_types: Option<KnownEventTypes>,
    extra_queries: Vec<Box<dyn Query<EventPublisher>>>,
    allow_local_network: bool,
}

impl Default for EventPublishingContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EventPublishingContextBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            views: None,
            event_bus: None,
            known_event_types: None,
            extra_queries: Vec::new(),
            allow_local_network: cfg!(feature = "allow-localhost"),
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn with_allow_local_network(mut self, allow: bool) -> Self {
        self.allow_local_network = allow;
        self
    }

    #[must_use]
    pub fn with_views(mut self, views: EventPublisherViewRepositories) -> Self {
        self.views = Some(views);
        self
    }

    #[must_use]
    pub fn with_event_bus(mut self, event_bus: EventBusHandle) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    #[must_use]
    pub fn with_known_event_types(mut self, known_event_types: KnownEventTypes) -> Self {
        self.known_event_types = Some(known_event_types);
        self
    }

    #[must_use]
    pub fn add_query(mut self, query: Box<dyn Query<EventPublisher>>) -> Self {
        self.extra_queries.push(query);
        self
    }

    /// Builds the [`EventPublishingContext`] and hydrates active event publisher workers
    /// by cross-checking entries in the persisted
    /// [`EventPublisherListView`](crate::application::views::event_publisher_list::EventPublisherListView)
    /// against their authoritative per-publisher views. Corrupt per-publisher rows
    /// (`PersistenceError::DeserializationError`) are logged and skipped rather than falling back
    /// to potentially stale list entries.
    ///
    /// # Errors
    ///
    /// Returns an error if required components (`views`, `event_bus`) are missing, if loading the
    /// persisted list view or per-publisher views fails with a database error, or if creating the
    /// command handler fails.
    pub async fn build(
        self,
        factory: &impl CommandHandlerFactory,
    ) -> Result<EventPublishingContext, Box<dyn std::error::Error>> {
        let views = self.views.ok_or("EventPublishingContextBuilder: views are required")?;
        let event_bus = self
            .event_bus
            .ok_or("EventPublishingContextBuilder: event_bus is required")?;
        let known_event_types = self.known_event_types.unwrap_or_default();

        let supervisor = EventPublisherSupervisor::new(event_bus).with_allow_local_network(self.allow_local_network);
        let mut authoritative_views = Vec::new();
        if let Some(list_view) = views.event_publisher_list.load(EVENT_PUBLISHER_LIST_VIEW_ID).await? {
            for (id, _) in list_view.into_entries() {
                match views.event_publisher.load(&id).await {
                    Ok(Some(view)) if !view.is_deleted() => authoritative_views.push(view),
                    Ok(Some(_)) => {
                        tracing::debug!(
                            publisher_id = %id,
                            "Skipping soft-deleted event publisher present in stale list view during startup hydration"
                        );
                    }
                    Ok(None) => {
                        tracing::warn!(
                            publisher_id = %id,
                            "Skipping event publisher from list view during startup hydration: per-publisher view not found"
                        );
                    }
                    Err(PersistenceError::DeserializationError(err)) => {
                        tracing::error!(
                            publisher_id = %id,
                            error = %err,
                            "Skipping corrupt event publisher view during startup hydration"
                        );
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }

        let domain_services = EventPublisherDomainServices::new(known_event_types)
            .with_allow_local_network_targets(self.allow_local_network);

        let mut publisher_query = GenericQuery::new(views.event_publisher.clone());
        publisher_query.use_error_handler(Box::new(|err| {
            tracing::error!(error = %err, "EventPublisher GenericQuery failed to update view");
        }));

        let mut queries: Vec<Box<dyn Query<EventPublisher>>> = vec![
            Box::new(publisher_query),
            Box::new(ListAllQuery::new(
                views.event_publisher_list.clone(),
                EVENT_PUBLISHER_LIST_VIEW_ID,
            )),
            Box::new(supervisor.clone()),
        ];
        queries.extend(self.extra_queries);

        let inner = factory.create_handler(domain_services, queries).await?;
        supervisor.hydrate_from_views(authoritative_views);
        let event_publisher = Arc::new(SerializedEventPublisherHandler {
            inner,
            execution: Arc::new(Mutex::new(())),
            supervisor: supervisor.clone(),
        });

        Ok(EventPublishingContext {
            event_publisher,
            views,
            supervisor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::views::{
            event_publisher::{EventPublisherView, Status},
            event_publisher_list::EventPublisherListView,
        },
        domain::event_publisher::{
            config::{EventPublisherConfig, EventPublisherFilter, HttpPublisherConfig},
            event::EventPublisherEvent,
        },
    };
    use chrono::Utc;
    use shared_kernel::{
        cqrs_es::{
            persist::{ViewContext, ViewRepository},
            EventEnvelope, View as _,
        },
        event_bus::CloudEvent,
        test_utils::in_memory::{InMemoryStore, MemViewRepository},
        view_repository::BoxedViewRepository,
    };
    use std::time::Duration;
    use tokio::sync::Notify;
    use tracing_test::traced_test;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn test_view(
        id: &str,
        target_url: String,
        enabled: bool,
        status: Status,
        filter: EventPublisherFilter,
        now: chrono::DateTime<Utc>,
    ) -> EventPublisherView {
        EventPublisherView {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            enabled,
            filter,
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
    async fn startup_hydration_cross_checks_list_entries_against_per_publisher_views() {
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

        let views = EventPublisherViewRepositories::from_factory(&InMemoryStore);
        let now = Utc::now();
        let filter = EventPublisherFilter {
            types: vec!["test.hydration.event".to_string()],
        };
        let stale_url = format!("{}/stale", mock_server.uri());
        let fresh_url = format!("{}/fresh", mock_server.uri());

        // Build a stale list view claiming all 4 publishers are enabled and pointing at `/stale`
        let mut stale_list = EventPublisherListView::default();
        for id in ["pub-deactivated", "pub-deleted", "pub-missing", "pub-updated"] {
            stale_list.update(&EventEnvelope {
                aggregate_id: id.to_string(),
                sequence: 1,
                payload: EventPublisherEvent::EventPublisherCreated {
                    id: id.to_string(),
                    name: id.to_string(),
                    description: None,
                    enabled: true,
                    filter: filter.clone(),
                    config: EventPublisherConfig::Http(HttpPublisherConfig {
                        target_url: stale_url.clone(),
                        headers: None,
                    }),
                    occurred_at: now,
                },
                metadata: HashMap::new(),
            });
        }
        views
            .event_publisher_list
            .update_view(
                stale_list,
                ViewContext::new(EVENT_PUBLISHER_LIST_VIEW_ID.to_string(), 0),
            )
            .await
            .unwrap();

        // Authoritative per-publisher views (pub-missing deliberately omitted):
        for (id, url, enabled, status) in [
            ("pub-deactivated", stale_url.clone(), false, Status::Configured),
            ("pub-deleted", stale_url, false, Status::None),
            ("pub-updated", fresh_url, true, Status::Configured),
        ] {
            views
                .event_publisher
                .update_view(
                    test_view(id, url, enabled, status, filter.clone(), now),
                    ViewContext::new(id.to_string(), 0),
                )
                .await
                .unwrap();
        }

        let event_bus = EventBusHandle::new(32);
        let context = EventPublishingContextBuilder::new()
            .with_views(views)
            .with_event_bus(event_bus.clone())
            .with_allow_local_network(true)
            .build(&InMemoryStore)
            .await
            .unwrap();

        assert_eq!(context.supervisor.active_count(), 1);
        assert!(!context.supervisor.is_active("pub-deactivated"));
        assert!(!context.supervisor.is_active("pub-deleted"));
        assert!(!context.supervisor.is_active("pub-missing"));
        assert!(context.supervisor.is_active("pub-updated"));

        event_bus.publish(CloudEvent::new("test.hydration.event", "/services/test"));
        wait_for_request_path(&mock_server, "/fresh", Duration::from_secs(2)).await;
        mock_server.verify().await;
    }

    #[derive(Clone, Copy)]
    enum LoadFault {
        Deserialization,
        Connection,
        Unknown,
    }

    struct FaultyViewRepository {
        inner: MemViewRepository<EventPublisherView, EventPublisher>,
        faults: HashMap<String, LoadFault>,
    }

    impl ViewRepository<EventPublisherView, EventPublisher> for FaultyViewRepository {
        async fn load(&self, view_id: &str) -> Result<Option<EventPublisherView>, PersistenceError> {
            if let Some(fault) = self.faults.get(view_id) {
                return Err(match fault {
                    LoadFault::Deserialization => {
                        PersistenceError::DeserializationError("corrupt publisher row".into())
                    }
                    LoadFault::Connection => PersistenceError::ConnectionError("database unreachable".into()),
                    LoadFault::Unknown => PersistenceError::UnknownError("unexpected db failure".into()),
                });
            }
            self.inner.load(view_id).await
        }

        async fn load_with_context(
            &self,
            view_id: &str,
        ) -> Result<Option<(EventPublisherView, ViewContext)>, PersistenceError> {
            self.inner.load_with_context(view_id).await
        }

        async fn update_view(&self, view: EventPublisherView, context: ViewContext) -> Result<(), PersistenceError> {
            self.inner.update_view(view, context).await
        }
    }

    async fn seeded_list_repo(
        ids: &[&str],
        target_url: &str,
        filter: &EventPublisherFilter,
        now: chrono::DateTime<Utc>,
    ) -> Arc<BoxedViewRepository<EventPublisherListView, EventPublisher>> {
        let mut list_view = EventPublisherListView::default();
        for id in ids {
            list_view.update(&EventEnvelope {
                aggregate_id: (*id).to_string(),
                sequence: 1,
                payload: EventPublisherEvent::EventPublisherCreated {
                    id: (*id).to_string(),
                    name: (*id).to_string(),
                    description: None,
                    enabled: true,
                    filter: filter.clone(),
                    config: EventPublisherConfig::Http(HttpPublisherConfig {
                        target_url: target_url.to_string(),
                        headers: None,
                    }),
                    occurred_at: now,
                },
                metadata: HashMap::new(),
            });
        }
        let list_repo = Arc::new(BoxedViewRepository::new(Box::new(MemViewRepository::<
            EventPublisherListView,
            EventPublisher,
        >::default())));
        list_repo
            .update_view(list_view, ViewContext::new(EVENT_PUBLISHER_LIST_VIEW_ID.to_string(), 0))
            .await
            .unwrap();
        list_repo
    }

    #[tokio::test]
    #[traced_test]
    async fn startup_hydration_skips_corrupt_view_without_falling_back_to_stale_list_entry() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/stale"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/healthy"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let now = Utc::now();
        let filter = EventPublisherFilter {
            types: vec!["test.corrupt.hydration".to_string()],
        };
        let stale_url = format!("{}/stale", mock_server.uri());
        let healthy_url = format!("{}/healthy", mock_server.uri());
        let list_repo = seeded_list_repo(&["pub-corrupt", "pub-healthy"], &stale_url, &filter, now).await;

        let inner_pub_repo = MemViewRepository::<EventPublisherView, EventPublisher>::default();
        inner_pub_repo
            .update_view(
                test_view("pub-healthy", healthy_url, true, Status::Configured, filter, now),
                ViewContext::new("pub-healthy".to_string(), 0),
            )
            .await
            .unwrap();

        let pub_repo = Arc::new(BoxedViewRepository::new(Box::new(FaultyViewRepository {
            inner: inner_pub_repo,
            faults: HashMap::from([("pub-corrupt".to_string(), LoadFault::Deserialization)]),
        })));
        let views = EventPublisherViewRepositories::new(pub_repo, list_repo);

        let event_bus = EventBusHandle::new(32);
        let context = EventPublishingContextBuilder::new()
            .with_views(views)
            .with_event_bus(event_bus.clone())
            .with_allow_local_network(true)
            .build(&InMemoryStore)
            .await
            .unwrap();

        assert_eq!(context.supervisor.active_count(), 1);
        assert!(!context.supervisor.is_active("pub-corrupt"));
        assert!(context.supervisor.is_active("pub-healthy"));
        assert!(logs_contain("ERROR"));
        assert!(logs_contain("pub-corrupt"));
        assert!(logs_contain(
            "Skipping corrupt event publisher view during startup hydration"
        ));

        event_bus.publish(CloudEvent::new("test.corrupt.hydration", "/services/test"));
        wait_for_request_path(&mock_server, "/healthy", Duration::from_secs(2)).await;
        mock_server.verify().await;
    }

    #[tokio::test]
    async fn startup_hydration_fails_fast_on_db_outage() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/healthy"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let now = Utc::now();
        let filter = EventPublisherFilter {
            types: vec!["test.outage.hydration".to_string()],
        };
        let healthy_url = format!("{}/healthy", mock_server.uri());
        let list_repo = seeded_list_repo(&["pub-healthy", "pub-outage"], &healthy_url, &filter, now).await;

        for outage_fault in [LoadFault::Connection, LoadFault::Unknown] {
            let inner_pub_repo = MemViewRepository::<EventPublisherView, EventPublisher>::default();
            inner_pub_repo
                .update_view(
                    test_view(
                        "pub-healthy",
                        healthy_url.clone(),
                        true,
                        Status::Configured,
                        filter.clone(),
                        now,
                    ),
                    ViewContext::new("pub-healthy".to_string(), 0),
                )
                .await
                .unwrap();

            let outage_pub_repo = Arc::new(BoxedViewRepository::new(Box::new(FaultyViewRepository {
                inner: inner_pub_repo,
                faults: HashMap::from([("pub-outage".to_string(), outage_fault)]),
            })));
            let outage_views = EventPublisherViewRepositories::new(outage_pub_repo, list_repo.clone());
            let event_bus = EventBusHandle::new(16);
            let result = EventPublishingContextBuilder::new()
                .with_views(outage_views)
                .with_event_bus(event_bus.clone())
                .with_allow_local_network(true)
                .build(&InMemoryStore)
                .await;
            assert!(result.is_err(), "expected DB outage during hydration to fail fast");

            event_bus.publish(CloudEvent::new("test.outage.hydration", "/services/test"));
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
        mock_server.verify().await;
    }

    struct FailingCommandHandlerFactory {
        event_bus: EventBusHandle,
    }

    impl CommandHandlerFactory for FailingCommandHandlerFactory {
        type Error = std::io::Error;

        async fn create_handler<A>(
            &self,
            _services: A::Services,
            _queries: Vec<Box<dyn Query<A>>>,
        ) -> Result<CommandHandler<A>, Self::Error>
        where
            A: shared_kernel::cqrs_es::Aggregate + 'static,
            <A as shared_kernel::cqrs_es::Aggregate>::Command: Send,
        {
            self.event_bus
                .publish(CloudEvent::new("test.handler.failure", "/services/test"));
            tokio::time::sleep(Duration::from_millis(100)).await;
            Err(std::io::Error::other("simulated command handler creation failure"))
        }
    }

    #[tokio::test]
    async fn build_does_not_start_workers_before_command_handler_creation_succeeds() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/should-not-start"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let now = Utc::now();
        let filter = EventPublisherFilter {
            types: vec!["test.handler.failure".to_string()],
        };
        let target_url = format!("{}/should-not-start", mock_server.uri());
        let list_repo = seeded_list_repo(&["pub-healthy"], &target_url, &filter, now).await;
        let pub_repo = Arc::new(BoxedViewRepository::new(Box::new(MemViewRepository::<
            EventPublisherView,
            EventPublisher,
        >::default())));
        pub_repo
            .update_view(
                test_view("pub-healthy", target_url, true, Status::Configured, filter, now),
                ViewContext::new("pub-healthy".to_string(), 0),
            )
            .await
            .unwrap();

        let event_bus = EventBusHandle::new(16);
        let result = EventPublishingContextBuilder::new()
            .with_views(EventPublisherViewRepositories::new(pub_repo, list_repo))
            .with_event_bus(event_bus.clone())
            .with_allow_local_network(true)
            .build(&FailingCommandHandlerFactory { event_bus })
            .await;

        assert!(result.is_err());
        mock_server.verify().await;
    }

    struct BlockingViewRepository {
        inner: MemViewRepository<EventPublisherView, EventPublisher>,
        blocked_id: String,
        block_once: std::sync::atomic::AtomicBool,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl ViewRepository<EventPublisherView, EventPublisher> for BlockingViewRepository {
        async fn load(&self, view_id: &str) -> Result<Option<EventPublisherView>, PersistenceError> {
            self.inner.load(view_id).await
        }

        async fn load_with_context(
            &self,
            view_id: &str,
        ) -> Result<Option<(EventPublisherView, ViewContext)>, PersistenceError> {
            self.inner.load_with_context(view_id).await
        }

        async fn update_view(&self, view: EventPublisherView, context: ViewContext) -> Result<(), PersistenceError> {
            if context.view_instance_id == self.blocked_id
                && self
                    .block_once
                    .compare_exchange(
                        true,
                        false,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    )
                    .is_ok()
            {
                self.entered.notify_one();
                self.release.notified().await;
            }
            self.inner.update_view(view, context).await
        }
    }

    struct CompletionSignalQuery {
        target_id: String,
        completed: Arc<Notify>,
    }

    #[async_trait]
    impl Query<EventPublisher> for CompletionSignalQuery {
        async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<EventPublisher>]) {
            if aggregate_id == self.target_id && !events.is_empty() {
                self.completed.notify_one();
            }
        }
    }

    fn create_http_command(id: &str, event_type: &str, target_url: String) -> EventPublisherCommand {
        EventPublisherCommand::CreateEventPublisher {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            enabled: true,
            filter: EventPublisherFilter {
                types: vec![event_type.to_string()],
            },
            config: EventPublisherConfig::Http(HttpPublisherConfig {
                target_url,
                headers: None,
            }),
        }
    }

    #[tokio::test]
    async fn caller_cancellation_after_commit_still_completes_views_and_supervisor() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let completed = Arc::new(Notify::new());

        let pub_repo = Arc::new(BoxedViewRepository::new(Box::new(BlockingViewRepository {
            inner: MemViewRepository::default(),
            blocked_id: "pub-cancel".to_string(),
            block_once: std::sync::atomic::AtomicBool::new(true),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        })));
        let list_repo = Arc::new(BoxedViewRepository::new(Box::new(MemViewRepository::<
            EventPublisherListView,
            EventPublisher,
        >::default())));
        let views = EventPublisherViewRepositories::new(pub_repo, list_repo);

        let event_bus = EventBusHandle::new(32);
        let context = EventPublishingContextBuilder::new()
            .with_views(views.clone())
            .with_event_bus(event_bus.clone())
            .with_allow_local_network(true)
            .add_query(Box::new(CompletionSignalQuery {
                target_id: "pub-cancel".to_string(),
                completed: Arc::clone(&completed),
            }))
            .build(&InMemoryStore)
            .await
            .unwrap();

        // Cancel (drop) the caller's future once the event is committed and GenericQuery enters
        // update_view (before ListAllQuery and EventPublisherSupervisor).
        {
            let create_fut = context.event_publisher.execute_with_metadata(
                "pub-cancel",
                create_http_command("pub-cancel", "test.cancel.event", format!("{}/hook", mock_server.uri())),
                HashMap::new(),
            );
            tokio::pin!(create_fut);

            tokio::select! {
                res = &mut create_fut => panic!("create completed before release: {res:?}"),
                () = entered.notified() => {}
            }
        }

        assert!(!context.supervisor.is_active("pub-cancel"));

        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), completed.notified())
            .await
            .expect("detached post-commit task should finish views and supervisor dispatch");

        assert!(context.supervisor.is_active("pub-cancel"));
        assert!(views.event_publisher.load("pub-cancel").await.unwrap().is_some());
        let list = views
            .event_publisher_list
            .load(EVENT_PUBLISHER_LIST_VIEW_ID)
            .await
            .unwrap()
            .expect("list view should be updated");
        assert!(list.into_entries().any(|(id, _)| id == "pub-cancel"));

        event_bus.publish(CloudEvent::new("test.cancel.event", "/services/test"));
        wait_for_request_path(&mock_server, "/hook", Duration::from_secs(2)).await;
        mock_server.verify().await;
    }

    struct RecordingQuery {
        target_id: String,
        events: Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl Query<EventPublisher> for RecordingQuery {
        async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<EventPublisher>]) {
            if aggregate_id != self.target_id {
                return;
            }
            let mut recorded = self.events.lock().unwrap();
            for event in events {
                let label = match event.payload {
                    EventPublisherEvent::EventPublisherCreated { .. } => "created",
                    EventPublisherEvent::EventPublisherUpdated { .. } => "updated",
                    EventPublisherEvent::EventPublisherActivated { .. } => "activated",
                    EventPublisherEvent::EventPublisherDeactivated { .. } => "deactivated",
                    EventPublisherEvent::EventPublisherDeleted { .. } => "deleted",
                };
                recorded.push(label);
            }
        }
    }

    #[tokio::test]
    async fn caller_cancellation_after_commit_keeps_execution_lock_until_post_commit_completes() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let recorded_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let pub_repo = Arc::new(BoxedViewRepository::new(Box::new(BlockingViewRepository {
            inner: MemViewRepository::default(),
            blocked_id: "pub-order".to_string(),
            block_once: std::sync::atomic::AtomicBool::new(true),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        })));
        let list_repo = Arc::new(BoxedViewRepository::new(Box::new(MemViewRepository::<
            EventPublisherListView,
            EventPublisher,
        >::default())));
        let context = EventPublishingContextBuilder::new()
            .with_views(EventPublisherViewRepositories::new(pub_repo, list_repo))
            .with_event_bus(EventBusHandle::new(16))
            .add_query(Box::new(RecordingQuery {
                target_id: "pub-order".to_string(),
                events: Arc::clone(&recorded_events),
            }))
            .build(&InMemoryStore)
            .await
            .unwrap();

        {
            let create_fut = context.event_publisher.execute_with_metadata(
                "pub-order",
                create_http_command("pub-order", "test.order.event", "https://example.test/hook".to_string()),
                HashMap::new(),
            );
            tokio::pin!(create_fut);
            tokio::select! {
                res = &mut create_fut => panic!("create completed before release: {res:?}"),
                () = entered.notified() => {}
            }
        }

        let deactivate_started = Arc::new(Notify::new());
        let deactivate_started_clone = Arc::clone(&deactivate_started);
        let handler = Arc::clone(&context.event_publisher);
        let deactivate_task = tokio::spawn(async move {
            deactivate_started_clone.notify_one();
            handler
                .execute_with_metadata(
                    "pub-order",
                    EventPublisherCommand::DeactivateEventPublisher,
                    HashMap::new(),
                )
                .await
        });

        // Wait until deactivate_task has definitely started and give it time to attempt lock acquisition.
        deactivate_started.notified().await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!deactivate_task.is_finished());
        assert_eq!(*recorded_events.lock().unwrap(), Vec::<&str>::new());

        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), deactivate_task)
            .await
            .expect("queued deactivate should finish after cancelled create")
            .unwrap()
            .unwrap();

        assert_eq!(*recorded_events.lock().unwrap(), vec!["created", "deactivated"]);
        assert!(!context.supervisor.is_active("pub-order"));
    }

    struct FixedResultExecutor {
        result: fn() -> Result<(), AggregateError<EventPublisherError>>,
    }

    #[async_trait]
    impl CommandExecutor<EventPublisher> for FixedResultExecutor {
        async fn execute_with_metadata(
            &self,
            _aggregate_id: &str,
            _command: EventPublisherCommand,
            _metadata: HashMap<String, String>,
        ) -> Result<(), AggregateError<EventPublisherError>> {
            (self.result)()
        }
    }

    #[tokio::test]
    async fn retry_reconciliation_stops_stale_worker_on_deactivate_ok_and_delete_not_found() {
        let now = Utc::now();
        let filter = EventPublisherFilter {
            types: vec!["test.reconcile.event".to_string()],
        };
        let target_url = "https://example.test/webhook".to_string();

        let views = EventPublisherViewRepositories::from_factory(&InMemoryStore);
        let context = EventPublishingContextBuilder::new()
            .with_views(views)
            .with_event_bus(EventBusHandle::new(32))
            .build(&InMemoryStore)
            .await
            .unwrap();

        // 1. DeactivateEventPublisher retry returns Ok(()) with no emitted events.
        context
            .event_publisher
            .execute_with_metadata(
                "pub-deact",
                create_http_command("pub-deact", "test.reconcile.event", target_url.clone()),
                HashMap::new(),
            )
            .await
            .unwrap();
        context
            .event_publisher
            .execute_with_metadata(
                "pub-deact",
                EventPublisherCommand::DeactivateEventPublisher,
                HashMap::new(),
            )
            .await
            .unwrap();

        context.supervisor.hydrate_from_views([test_view(
            "pub-deact",
            target_url.clone(),
            true,
            Status::Configured,
            filter.clone(),
            now,
        )]);
        assert!(context.supervisor.is_active("pub-deact"));

        context
            .event_publisher
            .execute_with_metadata(
                "pub-deact",
                EventPublisherCommand::DeactivateEventPublisher,
                HashMap::new(),
            )
            .await
            .unwrap();
        assert!(!context.supervisor.is_active("pub-deact"));

        // 2. DeleteEventPublisher retry returns EventPublisherNotFound with no emitted events.
        context
            .event_publisher
            .execute_with_metadata(
                "pub-del",
                create_http_command("pub-del", "test.reconcile.event", target_url.clone()),
                HashMap::new(),
            )
            .await
            .unwrap();
        context
            .event_publisher
            .execute_with_metadata("pub-del", EventPublisherCommand::DeleteEventPublisher, HashMap::new())
            .await
            .unwrap();

        context.supervisor.hydrate_from_views([test_view(
            "pub-del",
            target_url,
            true,
            Status::Configured,
            filter,
            now,
        )]);
        assert!(context.supervisor.is_active("pub-del"));

        let delete_retry = context
            .event_publisher
            .execute_with_metadata("pub-del", EventPublisherCommand::DeleteEventPublisher, HashMap::new())
            .await;
        assert!(matches!(
            delete_retry,
            Err(AggregateError::UserError(EventPublisherError::EventPublisherNotFound))
        ));
        assert!(!context.supervisor.is_active("pub-del"));
    }

    #[tokio::test]
    async fn delete_ok_reconciles_worker_even_without_supervisor_dispatch() {
        let now = Utc::now();
        let supervisor = EventPublisherSupervisor::new(EventBusHandle::new(16));
        supervisor.hydrate_from_views([test_view(
            "pub-del-ok",
            "https://example.test/webhook".to_string(),
            true,
            Status::Configured,
            EventPublisherFilter {
                types: vec!["test.reconcile.event".to_string()],
            },
            now,
        )]);
        assert!(supervisor.is_active("pub-del-ok"));

        let ok_handler = SerializedEventPublisherHandler {
            inner: Arc::new(FixedResultExecutor { result: || Ok(()) }),
            execution: Arc::new(Mutex::new(())),
            supervisor: supervisor.clone(),
        };
        ok_handler
            .execute_with_metadata(
                "pub-del-ok",
                EventPublisherCommand::DeleteEventPublisher,
                HashMap::new(),
            )
            .await
            .unwrap();
        assert!(!supervisor.is_active("pub-del-ok"));
    }

    #[tokio::test]
    async fn failed_deactivate_or_delete_does_not_stop_active_worker() {
        let now = Utc::now();
        let supervisor = EventPublisherSupervisor::new(EventBusHandle::new(16));
        supervisor.hydrate_from_views([test_view(
            "pub-err",
            "https://example.test/webhook".to_string(),
            true,
            Status::Configured,
            EventPublisherFilter {
                types: vec!["test.reconcile.event".to_string()],
            },
            now,
        )]);
        assert!(supervisor.is_active("pub-err"));

        let conflict_handler = SerializedEventPublisherHandler {
            inner: Arc::new(FixedResultExecutor {
                result: || Err(AggregateError::AggregateConflict),
            }),
            execution: Arc::new(Mutex::new(())),
            supervisor: supervisor.clone(),
        };

        assert!(matches!(
            conflict_handler
                .execute_with_metadata(
                    "pub-err",
                    EventPublisherCommand::DeactivateEventPublisher,
                    HashMap::new()
                )
                .await,
            Err(AggregateError::AggregateConflict)
        ));
        assert!(supervisor.is_active("pub-err"));

        assert!(matches!(
            conflict_handler
                .execute_with_metadata("pub-err", EventPublisherCommand::DeleteEventPublisher, HashMap::new())
                .await,
            Err(AggregateError::AggregateConflict)
        ));
        assert!(supervisor.is_active("pub-err"));

        let not_found_handler = SerializedEventPublisherHandler {
            inner: Arc::new(FixedResultExecutor {
                result: || Err(AggregateError::UserError(EventPublisherError::EventPublisherNotFound)),
            }),
            execution: Arc::new(Mutex::new(())),
            supervisor: supervisor.clone(),
        };
        assert!(matches!(
            not_found_handler
                .execute_with_metadata("pub-err", EventPublisherCommand::ActivateEventPublisher, HashMap::new())
                .await,
            Err(AggregateError::UserError(EventPublisherError::EventPublisherNotFound))
        ));
        assert!(supervisor.is_active("pub-err"));
    }
}
