use crate::{
    application::{
        access,
        command::{EventPublishingCommand, EventPublishingCommandError},
        query::{EventPublishingQuery, EventPublishingQueryError},
        supervisor::EventPublisherSupervisor,
        views::{
            event_publisher::EventPublisherView, event_publisher_list::EVENT_PUBLISHER_LIST_VIEW_ID,
            EventPublisherViewRepositories, EventPublishingView,
        },
    },
    domain::event_publisher::{aggregate::EventPublisher, services::KnownEventTypes},
};
use shared_kernel::{
    application_service::{ApplicationContext, ApplicationService},
    async_trait,
    authorization::Caller,
    command_handler::{self, CommandHandler},
    view_repository::{load_by_id, SoftDeletable as _},
};
use tracing::{debug, error, warn};

/// The main Application Service for the `EventPublishingContext`.
pub type EventPublishingService = ApplicationService<EventPublishingContext>;

#[derive(Clone)]
pub struct EventPublishingContext {
    pub event_publisher: CommandHandler<EventPublisher>,
    pub views: EventPublisherViewRepositories,
    pub supervisor: EventPublisherSupervisor,
    pub known_event_types: KnownEventTypes,
}

impl EventPublishingContext {
    async fn load_event_publisher(&self, id: &str) -> Result<Option<EventPublisherView>, EventPublishingQueryError> {
        Ok(load_by_id(self.views.event_publisher.as_ref(), id).await?)
    }

    async fn event_publisher_by_id(&self, id: &str) -> Result<EventPublishingView, EventPublishingQueryError> {
        if let Some(view) = self.load_event_publisher(id).await? {
            Ok(EventPublishingView::EventPublisher(Box::new(view.redacted())))
        } else {
            warn!(publisher_id = %id, "Event publisher not found or deleted");
            Err(EventPublishingQueryError::NotFound(id.to_string()))
        }
    }
}

#[async_trait]
impl ApplicationContext for EventPublishingContext {
    type Command = EventPublishingCommand;
    type Query = EventPublishingQuery;
    type View = EventPublishingView;

    type CommandError = EventPublishingCommandError;
    type QueryError = EventPublishingQueryError;

    async fn handle_command(
        &self,
        aggregate_id: &str,
        command: Self::Command,
        caller: &Caller,
    ) -> Result<String, Self::CommandError> {
        match command {
            EventPublishingCommand::EventPublisher { actor: _, command } => {
                command_handler::dispatch_with_caller(&self.event_publisher, aggregate_id, command, caller).await?;
            }
        }

        Ok(aggregate_id.to_string())
    }

    async fn handle_query(&self, query: Self::Query) -> Result<Self::View, Self::QueryError> {
        match query {
            EventPublishingQuery::GetEventPublisherById { id } => {
                debug!(publisher_id = %id, "Querying event publisher by ID");
                self.event_publisher_by_id(&id).await
            }
            EventPublishingQuery::GetEventPublisherList => {
                debug!("Querying event publisher list");
                match self.views.event_publisher_list.load(EVENT_PUBLISHER_LIST_VIEW_ID).await {
                    Ok(Some(view)) => {
                        let mut items: Vec<_> = view
                            .into_values()
                            .filter(|v| !v.is_deleted())
                            .map(EventPublisherView::redacted)
                            .collect();
                        items.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
                        debug!(count = items.len(), "Returning event publisher list");
                        Ok(EventPublishingView::EventPublisherList(items))
                    }
                    Ok(None) => {
                        debug!("No event publisher list view found, returning empty list");
                        Ok(EventPublishingView::EventPublisherList(vec![]))
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to load event publisher list");
                        Err(EventPublishingQueryError::PersistenceError(e))
                    }
                }
            }
        }
    }

    fn command_operation_name(&self, command: &Self::Command) -> &'static str {
        access::command_operation_name(command)
    }

    fn query_resource_id(&self, query: &Self::Query) -> Option<String> {
        match query {
            EventPublishingQuery::GetEventPublisherById { id } => Some(id.clone()),
            EventPublishingQuery::GetEventPublisherList => None,
        }
    }

    fn query_operation_name(&self, query: &Self::Query) -> &'static str {
        access::query_operation_name(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::builder::EventPublishingContextBuilder,
        domain::event_publisher::{
            command::EventPublisherCommand,
            config::{EventPublisherConfig, EventPublisherFilter, HttpPublisherConfig, REDACTED_HEADER_VALUE},
        },
    };
    use shared_kernel::{
        application_service::ApplicationContext,
        event_bus::{CloudEvent, EventBusHandle},
        test_utils::in_memory::InMemoryStore,
    };
    use std::{collections::BTreeMap, time::Duration};
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn runtime_lifecycle_create_update_deactivate_activate_delete_and_redaction() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .and(header("authorization", "Bearer top-secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(2)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(64);
        let views = EventPublisherViewRepositories::from_factory(&InMemoryStore);

        let context = EventPublishingContextBuilder::new()
            .with_views(views.clone())
            .with_event_bus(event_bus.clone())
            .build(&InMemoryStore)
            .await
            .unwrap();

        let pub_id = "pub-1".to_string();
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), "Bearer top-secret".to_string());

        // 1. Create active HTTP publisher
        context
            .handle_command(
                &pub_id,
                EventPublisherCommand::CreateEventPublisher {
                    id: pub_id.clone(),
                    name: "Live Webhook".to_string(),
                    description: Some("Initial description".to_string()),
                    enabled: true,
                    filter: EventPublisherFilter {
                        types: vec!["com.impierce.unicore.issuance.credential.issued".to_string()],
                    },
                    config: EventPublisherConfig::Http(HttpPublisherConfig {
                        target_url: format!("{}/hook", mock_server.uri()),
                        headers: Some(headers),
                    }),
                }
                .into(),
                &Caller::Internal,
            )
            .await
            .unwrap();

        assert_eq!(context.supervisor.active_count(), 1);
        assert!(context.supervisor.is_active(&pub_id));

        // Query returns redacted header value
        let EventPublishingView::EventPublisher(view) = context
            .handle_query(EventPublishingQuery::GetEventPublisherById { id: pub_id.clone() })
            .await
            .unwrap()
        else {
            panic!("expected single publisher view");
        };
        let Some(EventPublisherConfig::Http(redacted_http)) = view.config else {
            panic!("expected HTTP config");
        };
        assert_eq!(
            redacted_http
                .headers
                .as_ref()
                .unwrap()
                .get("authorization")
                .map(String::as_str),
            Some(REDACTED_HEADER_VALUE)
        );

        // Publish matching event -> delivered immediately
        event_bus.publish(CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        ));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);

        // 2. Deactivate publisher -> stops worker immediately
        context
            .handle_command(
                &pub_id,
                EventPublisherCommand::DeactivateEventPublisher { id: pub_id.clone() }.into(),
                &Caller::Internal,
            )
            .await
            .unwrap();
        assert!(!context.supervisor.is_active(&pub_id));

        event_bus.publish(CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mock_server.received_requests().await.unwrap().len(), 1);

        // 3. Reactivate publisher -> starts worker immediately with preserved secret header
        context
            .handle_command(
                &pub_id,
                EventPublisherCommand::ActivateEventPublisher { id: pub_id.clone() }.into(),
                &Caller::Internal,
            )
            .await
            .unwrap();
        assert!(context.supervisor.is_active(&pub_id));

        event_bus.publish(CloudEvent::new(
            "com.impierce.unicore.issuance.credential.issued",
            "/services/credential",
        ));
        tokio::time::sleep(Duration::from_millis(150)).await;
        mock_server.verify().await;

        // 4. Startup hydration from existing views starts active workers
        let restarted_context = EventPublishingContextBuilder::new()
            .with_views(views)
            .with_event_bus(event_bus)
            .build(&InMemoryStore)
            .await
            .unwrap();
        assert_eq!(restarted_context.supervisor.active_count(), 1);
        assert!(restarted_context.supervisor.is_active(&pub_id));
        drop(restarted_context);

        // 5. Delete publisher -> stops worker and removes from list
        context
            .handle_command(
                &pub_id,
                EventPublisherCommand::DeleteEventPublisher { id: pub_id.clone() }.into(),
                &Caller::Internal,
            )
            .await
            .unwrap();
        assert_eq!(context.supervisor.active_count(), 0);
        assert!(matches!(
            context
                .handle_query(EventPublishingQuery::GetEventPublisherById { id: pub_id })
                .await,
            Err(EventPublishingQueryError::NotFound(_))
        ));
    }
}
