use agent_shared::config::config;
use shared_kernel::event_bus::{EventBus, EventBusHandle, EventFilter};
use tokio_stream::StreamExt;
use tracing::info;

/// Spawns a background worker that subscribes to the [`EventBusHandle`] and forwards
/// canonical [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s to configured NATS subjects.
pub fn start_nats_forwarder(event_bus: EventBusHandle) -> Option<tokio::task::JoinHandle<()>> {
    let conf = config();
    let nats_config = conf.event_publishers.nats.as_ref()?;
    if !nats_config.enabled {
        return None;
    }

    let nats_url = nats_config.nats_url.clone();
    let subjects = nats_config.subjects.clone();
    let mut stream = event_bus.subscribe(EventFilter::default());

    Some(tokio::spawn(async move {
        info!("Connecting NATS event publisher forwarder to {}...", nats_url);
        let client = match async_nats::connect(&nats_url).await {
            Ok(c) => c,
            Err(err) => {
                tracing::error!("Failed to connect to NATS at {}: {:?}", nats_url, err);
                return;
            }
        };

        info!("NATS event publisher forwarder connected successfully.");

        while let Some(item) = stream.next().await {
            if let Ok(cloud_event) = item {
                for subject_config in &subjects {
                    let filter = EventFilter {
                        event_types: subject_config.events.types.clone(),
                        ..Default::default()
                    };
                    if !filter.matches(&cloud_event) {
                        continue;
                    }

                    let subject_name = subject_config.name.clone();
                    let payload = match serde_json::to_vec(&cloud_event) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("Failed to serialize CloudEvent for NATS: {:?}", e);
                            continue;
                        }
                    };

                    if let Err(err) = client.publish(subject_name.clone(), payload.into()).await {
                        tracing::error!(
                            "Failed to publish CloudEvent {:?} to NATS subject {}: {:?}",
                            cloud_event.id,
                            subject_name,
                            err
                        );
                    } else {
                        info!(
                            "Published CloudEvent {:?} to NATS subject {}",
                            cloud_event.id, subject_name
                        );
                    }
                }
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::config::{set_config, EventPublisherNats, Events, NatsSubject};
    use shared_kernel::event_bus::CloudEvent;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::const_new(());

    #[tokio::test]
    async fn test_start_nats_forwarder_disabled() {
        let _guard = TEST_MUTEX.lock().await;

        {
            let mut conf = set_config();
            conf.event_publishers.nats = None;
        }

        let event_bus = EventBusHandle::new(100);
        let handle = start_nats_forwarder(event_bus.clone());
        assert!(handle.is_none(), "Expected None when NATS publisher config is None");

        {
            let mut conf = set_config();
            conf.event_publishers.nats = Some(EventPublisherNats {
                enabled: false,
                nats_url: "127.0.0.1:4222".to_string(),
                subjects: vec![],
            });
        }

        let handle = start_nats_forwarder(event_bus);
        assert!(handle.is_none(), "Expected None when NATS publisher enabled flag is false");

        // Cleanup
        set_config().event_publishers.nats = None;
    }

    #[tokio::test]
    async fn test_start_nats_forwarder_connection_failure() {
        let _guard = TEST_MUTEX.lock().await;

        // Use a closed port that immediately fails connection
        {
            let mut conf = set_config();
            conf.event_publishers.nats = Some(EventPublisherNats {
                enabled: true,
                nats_url: "127.0.0.1:1".to_string(),
                subjects: vec![NatsSubject {
                    name: "test.subject".to_string(),
                    events: Events {
                        types: vec!["*".to_string()],
                    },
                }],
            });
        }

        let event_bus = EventBusHandle::new(100);
        let handle = start_nats_forwarder(event_bus)
            .expect("Expected JoinHandle when NATS publisher is enabled");

        // The background task should exit gracefully when connect fails
        let res = tokio::time::timeout(Duration::from_secs(3), handle).await;
        assert!(res.is_ok(), "Task should finish promptly on connection failure");

        // Cleanup
        set_config().event_publishers.nats = None;
    }

    #[tokio::test]
    async fn test_start_nats_forwarder_publishes_to_mock_nats() {
        let _guard = TEST_MUTEX.lock().await;

        // Bind mock NATS TCP listener on an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let (received_sender, mut received_receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(10);

        // Spawn mock NATS server task
        let mock_server_handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                // 1. Send NATS server INFO greeting
                let info = b"INFO {\"server_id\":\"TEST\",\"server_name\":\"test\",\"version\":\"2.10.0\",\"proto\":1,\"headers\":true,\"max_payload\":1048576}\r\n";
                let _ = socket.write_all(info).await;

                let mut buf = vec![0u8; 8192];
                loop {
                    let n = match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let chunk = &buf[..n];

                    // Respond to PING with PONG
                    if chunk.windows(4).any(|w| w == b"PING") {
                        let _ = socket.write_all(b"PONG\r\n").await;
                    }

                    // Forward published payloads (PUB / HPUB)
                    if chunk.starts_with(b"PUB") || chunk.starts_with(b"HPUB") {
                        let _ = received_sender.send(chunk.to_vec()).await;
                    }
                }
            }
        });

        {
            let mut conf = set_config();
            conf.event_publishers.nats = Some(EventPublisherNats {
                enabled: true,
                nats_url: local_addr.to_string(),
                subjects: vec![NatsSubject {
                    name: "unicore.events".to_string(),
                    events: Events {
                        types: vec!["tech.impierce.unicore.credential.issued".to_string()],
                    },
                }],
            });
        }

        let event_bus = EventBusHandle::new(100);
        let handle = start_nats_forwarder(event_bus.clone())
            .expect("Expected JoinHandle when NATS publisher is enabled");

        // Wait briefly for NATS handshake to complete
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 1. Publish non-matching event
        let non_matching = CloudEvent::new("org.other.event", "https://example.com/other");
        event_bus.publish(non_matching);

        // 2. Publish matching event
        let matching = CloudEvent::new(
            "tech.impierce.unicore.credential.issued",
            "https://example.com/issuer",
        )
        .with_subject("sub-42")
        .with_caller(Some("caller-nats".to_string()), Some("api_key".to_string()))
        .with_data(serde_json::json!({ "status": "issued" }));

        let matching_id = matching.id.clone();
        event_bus.publish(matching);

        // Receive the message published to mock NATS
        let received = tokio::time::timeout(Duration::from_secs(3), received_receiver.recv())
            .await
            .expect("Mock NATS should receive published message")
            .expect("Channel should not be closed");

        let received_str = String::from_utf8_lossy(&received);
        // Verify subject was published to
        assert!(
            received_str.contains("unicore.events"),
            "Expected NATS message to contain subject 'unicore.events', got: {}",
            received_str
        );

        // Verify JSON payload inside received message
        assert!(received_str.contains(&matching_id));
        assert!(received_str.contains("caller-nats"));
        assert!(received_str.contains("tech.impierce.unicore.credential.issued"));

        handle.abort();
        mock_server_handle.abort();
        set_config().event_publishers.nats = None;
    }
}

