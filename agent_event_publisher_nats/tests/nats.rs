//! Integration test against a NATS server running in Docker (`docker-tests` feature): configured offer events are
//! published as CloudEvents to the configured subject.

use agent_event_publisher_nats::EventPublisherNats;
use agent_issuance::offer::{
    aggregate::{DeliveryOptions, Offer, Status},
    event::OfferEvent,
};
use agent_shared::config::{self, set_config, Events, NatsSubject};
use agent_store::EventPublisher as _;
use cqrs_es::EventEnvelope;
use futures::StreamExt as _;
use serde_json::Value;
use std::time::Duration;
use testcontainers_modules::{
    nats::Nats,
    testcontainers::{runners::AsyncRunner, ImageExt},
};

const SUBJECT: &str = "unicore.offers";

/// Sets the NATS event publisher configuration. Each file in `tests/` runs in its own process, so this only affects
/// the configuration of this test binary.
fn configure_nats(nats: Option<config::EventPublisherNats>) {
    set_config().event_publishers.nats = nats;
}

fn nats_config(enabled: bool, nats_url: &str) -> config::EventPublisherNats {
    config::EventPublisherNats {
        enabled,
        nats_url: nats_url.to_string(),
        subjects: vec![NatsSubject {
            name: SUBJECT.to_string(),
            events: Events {
                offer: vec![config::OfferEvent::TxCodeGenerated],
                ..Default::default()
            },
        }],
    }
}

fn envelope(payload: OfferEvent) -> EventEnvelope<Offer> {
    EventEnvelope {
        aggregate_id: "offer-1".to_string(),
        sequence: 1,
        payload,
        metadata: Default::default(),
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

    // Without an enabled configuration, nothing is published.
    configure_nats(None);
    assert!(EventPublisherNats::load().await.unwrap().offer.is_none());
    configure_nats(Some(nats_config(false, &nats_url)));
    assert!(EventPublisherNats::load().await.unwrap().offer.is_none());

    // An unreachable server fails loading instead of silently dropping events.
    configure_nats(Some(nats_config(true, "nats://127.0.0.1:1")));
    assert!(EventPublisherNats::load().await.is_err());

    configure_nats(Some(nats_config(true, &nats_url)));
    let mut publishers = EventPublisherNats::load().await.unwrap();
    let publisher = publishers.offer.take().unwrap();
    assert_eq!(publisher.target_events, vec!["TxCodeGenerated".to_string()]);

    // Only the `Offer` aggregate is published to NATS.
    let mut publishers = EventPublisherNats { offer: Some(publisher) };
    let offer_publisher = publishers.offer().unwrap();
    assert!(publishers.offer().is_none());
    assert!(publishers.credential().is_none());
    assert!(publishers.server_config().is_none());
    assert!(publishers.template().is_none());

    let client = async_nats::connect(&nats_url).await.unwrap();
    let mut subscriber = client.subscribe(SUBJECT).await.unwrap();
    client.flush().await.unwrap();

    // An event that is not configured is not published; the configured one is.
    offer_publisher
        .dispatch(
            "offer-1",
            &[
                envelope(OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: "offer-1".to_string(),
                    form_url_encoded_credential_offer: "openid-credential-offer://?credential_offer=...".to_string(),
                    status: Status::Pending,
                }),
                envelope(OfferEvent::TxCodeGenerated {
                    offer_id: "offer-1".to_string(),
                    tx_code: "12345".to_string(),
                    delivery_options: Some(DeliveryOptions {
                        recipient_email: Some("holder@example.test".to_string()),
                    }),
                }),
            ],
        )
        .await;

    let message = tokio::time::timeout(Duration::from_secs(10), subscriber.next())
        .await
        .expect("no event was published within 10 seconds")
        .unwrap();
    let cloud_event: Value = serde_json::from_slice(&message.payload).unwrap();
    assert_eq!(cloud_event["type"], "offer.event.txcodegenerated");
    assert!(cloud_event["id"].as_str().unwrap().starts_with("offer-1-"));
    assert_eq!(cloud_event["data"]["TxCodeGenerated"]["tx_code"], "12345");

    assert!(
        tokio::time::timeout(Duration::from_millis(500), subscriber.next())
            .await
            .is_err(),
        "an event that is not configured was published"
    );
}
