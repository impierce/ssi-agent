use crate::{
    application::{
        service::EventPublishingContext,
        supervisor::EventPublisherSupervisor,
        views::{event_publisher_list::EVENT_PUBLISHER_LIST_VIEW_ID, EventPublisherViewRepositories},
    },
    domain::event_publisher::{
        aggregate::EventPublisher,
        services::{EventPublisherDomainServices, KnownEventTypes},
    },
};
use shared_kernel::{
    command_handler::CommandHandlerFactory,
    cqrs_es::{persist::GenericQuery, Query},
    custom_queries::ListAllQuery,
    event_bus::EventBusHandle,
};
use tracing::{debug, info};

#[derive(Default)]
pub struct EventPublishingContextBuilder {
    views: Option<EventPublisherViewRepositories>,
    event_bus: Option<EventBusHandle>,
    known_event_types: Option<KnownEventTypes>,
    extra_queries: Vec<Box<dyn Query<EventPublisher>>>,
}

impl EventPublishingContextBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        info!("Building EventPublishingContext");

        let views = self.views.ok_or("EventPublishingContextBuilder: views are required")?;
        let event_bus = self
            .event_bus
            .ok_or("EventPublishingContextBuilder: event_bus is required")?;
        let known_event_types = self.known_event_types.unwrap_or_default();

        let supervisor = EventPublisherSupervisor::new(event_bus);
        if let Some(list_view) = views.event_publisher_list.load(EVENT_PUBLISHER_LIST_VIEW_ID).await? {
            supervisor.hydrate_from_views(list_view.into_values());
        }

        let domain_services = EventPublisherDomainServices::new(known_event_types.clone());

        let mut queries: Vec<Box<dyn Query<EventPublisher>>> = vec![
            Box::new(GenericQuery::new(views.event_publisher.clone())),
            Box::new(ListAllQuery::new(
                views.event_publisher_list.clone(),
                EVENT_PUBLISHER_LIST_VIEW_ID,
            )),
            Box::new(supervisor.clone()),
        ];
        queries.extend(self.extra_queries);

        debug!(query_count = queries.len(), "Creating event_publisher command handler");
        let event_publisher = factory.create_handler(domain_services, queries).await?;

        info!("EventPublishingContext built successfully");
        Ok(EventPublishingContext {
            event_publisher,
            views,
            supervisor,
            known_event_types,
        })
    }
}
