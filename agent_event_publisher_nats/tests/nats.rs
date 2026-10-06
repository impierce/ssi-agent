//! Integration test against a NATS server running in Docker (`docker-tests` feature): configured events are
//! published as CloudEvents to the configured subject.

use agent_event_publisher_nats::NatsEventPublisher;
use agent_shared::config::{set_config, EventPublisherNats, Events, NatsSubject};
use futures::StreamExt as _;
use serde_json::Value;
use shared_kernel::event_bus::{CloudEvent, EventBusHandle};
use std::time::Duration;
use testcontainers_modules::{
    nats::Nats,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

const SUBJECT: &str = "unicore.offers";

/// Sets the NATS event publisher configuration. Each file in `tests/` runs in its own process, so this only affects
/// the configuration of this test binary.
fn configure_nats(nats: Option<EventPublisherNats>) {
    set_config().event_publishers.nats = nats;
}

fn nats_config(enabled: bool, nats_url: &str) -> EventPublisherNats {
    EventPublisherNats {
        enabled,
        nats_url: nats_url.to_string(),
        subjects: vec![NatsSubject {
            name: SUBJECT.to_string(),
            events: Events {
                types: vec!["TxCodeGenerated".to_string()],
            },
        }],
    }
}

#[tokio::test]
async fn configured_offer_events_are_published_as_cloud_events() {
    let container = Nats::default().with_tag("2").start().await.unwrap();
    let nats_url = format!(
        "nats://{}:{}",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(4222).await.unwrap()
    );

    let event_bus = EventBusHandle::new(100);

    // Without an enabled configuration, nothing is loaded.
    configure_nats(None);
    assert!(NatsEventPublisher::from_config(&event_bus).is_none());
    configure_nats(Some(nats_config(false, &nats_url)));
    assert!(NatsEventPublisher::from_config(&event_bus).is_none());

    // Configure the publisher against the running NATS container
    configure_nats(Some(nats_config(true, &nats_url)));
    let publisher = NatsEventPublisher::from_config(&event_bus).expect("publisher should load when enabled");
    let publisher_handle = publisher.spawn();

    // Subscribe to NATS subject
    let client = async_nats::connect(&nats_url).await.unwrap();
    let mut subscriber = client.subscribe(SUBJECT).await.unwrap();
    client.flush().await.unwrap();

    // Allow a short moment for publisher background task connection to establish
    tokio::time::sleep(Duration::from_millis(200)).await;

    // An event that is not configured is not published
    let unconfigured_event = CloudEvent::new(
        "com.impierce.unicore.form-url-encoded-credential-offer-created",
        "https://impierce.com/offer",
    )
    .with_subject("offer-1");
    event_bus.publish(unconfigured_event);

    // The configured event is published
    let configured_event = CloudEvent::new(
        "com.impierce.unicore.tx-code-generated",
        "https://impierce.com/offer",
    )
    .with_subject("offer-1")
    .with_data(serde_json::json!({
        "TxCodeGenerated": {
            "offer_id": "offer-1",
            "tx_code": "12345",
            "delivery_options": {
                "recipient_email": "holder@example.test"
            }
        }
    }));
    let expected_event_id = configured_event.id.clone();
    event_bus.publish(configured_event);

    let message = tokio::time::timeout(Duration::from_secs(10), subscriber.next())
        .await
        .expect("no event was published within 10 seconds")
        .unwrap();
    let cloud_event: Value = serde_json::from_slice(&message.payload).unwrap();
    assert_eq!(cloud_event["type"], "com.impierce.unicore.tx-code-generated");
    assert_eq!(cloud_event["id"], expected_event_id);
    assert_eq!(cloud_event["data"]["TxCodeGenerated"]["tx_code"], "12345");

    assert!(
        tokio::time::timeout(Duration::from_millis(500), subscriber.next())
            .await
            .is_err(),
        "an event that is not configured was published"
    );

    publisher_handle.abort();
}
