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
    cqrs_es::{persist::GenericQuery, AggregateError, Query},
    custom_queries::ListAllQuery,
    event_bus::EventBusHandle,
    view_repository::SoftDeletable as _,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

/// Keeps the aggregate load, event write, view projections, and [`EventPublisherSupervisor`]
/// worker transitions serialized within one process so concurrent commands cannot dispatch
/// out of order to `ListAllQuery` or `EventPublisherSupervisor`.
struct SerializedEventPublisherHandler {
    inner: CommandHandler<EventPublisher>,
    execution: Mutex<()>,
}

#[async_trait]
impl CommandExecutor<EventPublisher> for SerializedEventPublisherHandler {
    async fn execute_with_metadata(
        &self,
        aggregate_id: &str,
        command: EventPublisherCommand,
        metadata: HashMap<String, String>,
    ) -> Result<(), AggregateError<EventPublisherError>> {
        let _execution = self.execution.lock().await;
        self.inner.execute_with_metadata(aggregate_id, command, metadata).await
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
    /// from the persisted [`EventPublisherListView`](crate::application::views::event_publisher_list::EventPublisherListView).
    ///
    /// # Errors
    ///
    /// Returns an error if required components (`views`, `event_bus`) are missing, or if creating
    /// the command handler or loading the persisted list view fails.
    pub async fn build(
        self,
        factory: &impl CommandHandlerFactory,
    ) -> Result<EventPublishingContext, Box<dyn std::error::Error>> {
        let views = self.views.ok_or("EventPublishingContextBuilder: views are required")?;
        let event_bus = self
            .event_bus
            .ok_or("EventPublishingContextBuilder: event_bus is required")?;
        let known_event_types = self.known_event_types.unwrap_or_default();

        let supervisor = EventPublisherSupervisor::new(event_bus)
            .with_view_repository(views.event_publisher.clone())
            .with_allow_local_network(self.allow_local_network);
        if let Some(list_view) = views.event_publisher_list.load(EVENT_PUBLISHER_LIST_VIEW_ID).await? {
            let mut authoritative_views = Vec::new();
            for (id, _) in list_view.into_entries() {
                match views.event_publisher.load(&id).await? {
                    Some(view) if !view.is_deleted() => authoritative_views.push(view),
                    Some(_) => {
                        tracing::debug!(
                            publisher_id = %id,
                            "Skipping soft-deleted event publisher present in stale list view during startup hydration"
                        );
                    }
                    None => {
                        tracing::warn!(
                            publisher_id = %id,
                            "Skipping event publisher from list view during startup hydration: per-publisher view not found"
                        );
                    }
                }
            }
            supervisor.hydrate_from_views(authoritative_views);
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
        let event_publisher = Arc::new(SerializedEventPublisherHandler {
            inner,
            execution: Mutex::new(()),
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
            persist::{ViewContext, ViewRepository as _},
            EventEnvelope, View as _,
        },
        event_bus::CloudEvent,
        test_utils::in_memory::InMemoryStore,
    };
    use std::time::Duration;
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
        tokio::time::sleep(Duration::from_millis(150)).await;
        mock_server.verify().await;
    }
}
