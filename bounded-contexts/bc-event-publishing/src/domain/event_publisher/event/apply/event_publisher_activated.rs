use crate::domain::event_publisher::{
    aggregate::EventPublisher,
    config::{EventPublisherConfig, EventPublisherFilter},
};
use chrono::{DateTime, Utc};

impl EventPublisher {
    pub fn event_publisher_activated(
        &mut self,
        filter: EventPublisherFilter,
        config: EventPublisherConfig,
        occurred_at: DateTime<Utc>,
    ) {
        if let EventPublisher::Configured(inner) = self {
            inner.enabled = true;
            inner.filter = filter;
            inner.config = config;
            inner.last_modified_at = occurred_at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::event_publisher::{
        config::HttpPublisherConfig,
        event::{apply::event_publisher_created::tests::created_http_event, EventPublisherEvent},
    };
    use rstest::rstest;
    use shared_kernel::cqrs_es::Aggregate as _;

    #[rstest]
    fn applying_activated_updates_enabled_filter_config_and_timestamp(created_http_event: EventPublisherEvent) {
        let mut aggregate = EventPublisher::default();
        aggregate.apply(created_http_event);
        aggregate.apply(EventPublisherEvent::EventPublisherDeactivated {
            occurred_at: Utc::now(),
        });

        let activated_at = Utc::now() + chrono::Duration::seconds(5);
        let new_filter = EventPublisherFilter {
            types: vec!["com.impierce.unicore.verification.*".to_string()],
        };
        let new_config = EventPublisherConfig::Http(HttpPublisherConfig {
            target_url: "https://example.test/webhooks/activated".to_string(),
            headers: None,
        });

        aggregate.apply(EventPublisherEvent::EventPublisherActivated {
            filter: new_filter.clone(),
            config: new_config.clone(),
            occurred_at: activated_at,
        });

        let EventPublisher::Configured(inner) = aggregate else {
            panic!("expected configured aggregate");
        };
        assert!(inner.enabled);
        assert_eq!(inner.filter, new_filter);
        assert_eq!(inner.config, new_config);
        assert_eq!(inner.last_modified_at, activated_at);

        // Applying to EventPublisher::None remains None.
        let mut empty = EventPublisher::default();
        empty.apply(EventPublisherEvent::EventPublisherActivated {
            filter: new_filter,
            config: new_config,
            occurred_at: activated_at,
        });
        assert!(empty.is_none());
    }
}
