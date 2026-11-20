pub mod event_publisher;
pub mod event_publisher_list;

use crate::{
    application::views::{
        event_publisher::EventPublisherView,
        event_publisher_list::{EventPublisherListView, EVENT_PUBLISHER_LIST_VIEW_ID},
    },
    domain::event_publisher::aggregate::EventPublisher,
};
use shared_kernel::view_repository::{BoxedViewRepository, ViewRepositoryFactory};
use std::sync::Arc;

#[derive(Clone)]
pub struct EventPublisherViewRepositories {
    pub(crate) event_publisher: Arc<BoxedViewRepository<EventPublisherView, EventPublisher>>,
    pub(crate) event_publisher_list: Arc<BoxedViewRepository<EventPublisherListView, EventPublisher>>,
}

impl EventPublisherViewRepositories {
    #[must_use]
    pub fn new(
        event_publisher: Arc<BoxedViewRepository<EventPublisherView, EventPublisher>>,
        event_publisher_list: Arc<BoxedViewRepository<EventPublisherListView, EventPublisher>>,
    ) -> Self {
        Self {
            event_publisher,
            event_publisher_list,
        }
    }

    pub fn from_factory(factory: &impl ViewRepositoryFactory) -> Self {
        Self {
            event_publisher: Arc::new(factory.create_view_repository("event_publisher")),
            event_publisher_list: Arc::new(factory.create_view_repository(EVENT_PUBLISHER_LIST_VIEW_ID)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventPublishingView {
    EventPublisher(Box<EventPublisherView>),
    EventPublisherList(Vec<EventPublisherView>),
}
