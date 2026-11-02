use crate::domain::event_publisher::config::NatsPublisherConfig;
use async_nats::Client;
use shared_kernel::event_bus::{BusEventStream, EventBus, EventBusError, EventBusHandle, EventFilter};
use std::time::Duration;
use tokio_stream::StreamExt;
use tracing::info;

/// Background worker that subscribes to the [`EventBusHandle`] for a single configured
/// NATS event publisher and publishes matching [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s
/// to the configured NATS subject.
pub struct NatsEventPublisherWorker {
    id: String,
    client: Client,
    subject: String,
    filter: EventFilter,
    stream: BusEventStream,
}

impl NatsEventPublisherWorker {
    /// Connects to the NATS server in `config` and creates a `NatsEventPublisherWorker`.
    ///
    /// Subscribes immediately to `event_bus` before connecting so no events published
    /// during connection establishment are lost.
    ///
    /// # Errors
    ///
    /// Returns [`async_nats::ConnectError`] if connecting to `config.nats_url` fails.
    pub async fn connect(
        id: String,
        events: Vec<String>,
        config: NatsPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> Result<Self, async_nats::ConnectError> {
        let filter = EventFilter {
            event_types: events,
            ..Default::default()
        };
        let stream = event_bus.subscribe(filter.clone());

        info!(
            publisher_id = %id,
            nats_url = %config.nats_url,
            subject = %config.subject,
            "Connecting NATS event publisher worker"
        );
        let client = async_nats::connect(&config.nats_url).await?;
        info!(publisher_id = %id, "NATS event publisher worker connected successfully");

        Ok(Self {
            id,
            client,
            subject: config.subject,
            filter,
            stream,
        })
    }

    /// Spawns a resilient NATS worker task that subscribes immediately to `event_bus` and
    /// connects to `config.nats_url` in the background (retrying with exponential backoff
    /// if the broker is temporarily unreachable) so runtime commands and startup hydration
    /// never block or fail when a NATS broker is restarting.
    #[must_use]
    pub fn spawn_resilient(
        id: String,
        events: Vec<String>,
        config: NatsPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> tokio::task::JoinHandle<()> {
        let filter = EventFilter {
            event_types: events,
            ..Default::default()
        };
        let stream = event_bus.subscribe(filter.clone());

        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            let client = loop {
                match async_nats::connect(&config.nats_url).await {
                    Ok(client) => {
                        info!(
                            publisher_id = %id,
                            nats_url = %config.nats_url,
                            subject = %config.subject,
                            "NATS event publisher worker connected"
                        );
                        break client;
                    }
                    Err(err) => {
                        tracing::error!(
                            publisher_id = %id,
                            nats_url = %config.nats_url,
                            error = ?err,
                            retry_in_secs = backoff.as_secs(),
                            "Failed to connect NATS event publisher worker; retrying"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                    }
                }
            };

            let worker = Self {
                id,
                client,
                subject: config.subject,
                filter,
                stream,
            };
            worker.run().await;
        })
    }

    /// Runs the NATS event publisher loop until the stream closes or the task is aborted.
    pub async fn run(mut self) {
        while let Some(item) = self.stream.next().await {
            let cloud_event = match item {
                Ok(event) => event,
                Err(EventBusError::Lagged(dropped_count)) => {
                    tracing::warn!(
                        publisher_id = %self.id,
                        dropped_count,
                        "NATS event publisher worker lagged behind; events were dropped"
                    );
                    continue;
                }
                Err(err) => {
                    tracing::error!(
                        publisher_id = %self.id,
                        error = ?err,
                        "NATS event publisher worker encountered event bus error"
                    );
                    continue;
                }
            };

            if self.filter.event_types.is_empty() || !self.filter.matches(&cloud_event) {
                continue;
            }

            let payload = match serde_json::to_vec(&cloud_event) {
                Ok(p) => p,
                Err(err) => {
                    tracing::error!(
                        publisher_id = %self.id,
                        error = ?err,
                        "Failed to serialize CloudEvent for NATS"
                    );
                    continue;
                }
            };

            if let Err(err) = self.client.publish(self.subject.clone(), payload.into()).await {
                tracing::error!(
                    publisher_id = %self.id,
                    event_id = %cloud_event.id,
                    subject = %self.subject,
                    error = ?err,
                    "Failed to publish CloudEvent to NATS subject"
                );
            } else {
                info!(
                    publisher_id = %self.id,
                    event_id = %cloud_event.id,
                    subject = %self.subject,
                    "Published CloudEvent to NATS subject"
                );
            }
        }
    }

    /// Spawns the connected NATS event publisher worker on the Tokio runtime.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_kernel::event_bus::CloudEvent;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn nats_worker_connection_failure_returns_err() {
        let event_bus = EventBusHandle::new(100);
        let config = NatsPublisherConfig {
            nats_url: "127.0.0.1:1".to_string(),
            subject: "test.subject".to_string(),
        };

        let res = NatsEventPublisherWorker::connect(
            "nats-1".to_string(),
            vec!["com.impierce.unicore.issuance.credential.offered".to_string()],
            config,
            &event_bus,
        )
        .await;
        assert!(res.is_err(), "Expected connection to fail when server is unreachable");
    }

    #[tokio::test]
    async fn nats_worker_publishes_to_mock_nats() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let (received_sender, mut received_receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(10);

        let mock_server_handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let info = b"INFO {\"server_id\":\"TEST\",\"server_name\":\"test\",\"version\":\"2.10.0\",\"proto\":1,\"headers\":true,\"max_payload\":1048576}\r\n";
                let _ = socket.write_all(info).await;

                let mut buf = vec![0u8; 8192];
                loop {
                    let n = match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let chunk = &buf[..n];

                    if chunk.windows(4).any(|w| w == b"PING") {
                        let _ = socket.write_all(b"PONG\r\n").await;
                    }

                    if chunk.starts_with(b"PUB") || chunk.starts_with(b"HPUB") {
                        let _ = received_sender.send(chunk.to_vec()).await;
                    }
                }
            }
        });

        let config = NatsPublisherConfig {
            nats_url: local_addr.to_string(),
            subject: "unicore.events".to_string(),
        };

        let event_bus = EventBusHandle::new(100);
        let worker = NatsEventPublisherWorker::connect(
            "nats-1".to_string(),
            vec!["com.impierce.unicore.credential.issued".to_string()],
            config,
            &event_bus,
        )
        .await
        .unwrap();
        let handle = worker.spawn();

        tokio::time::sleep(Duration::from_millis(150)).await;

        // 1. Non-matching event
        event_bus.publish(CloudEvent::new("org.other.event", "https://example.com/other"));

        // 2. Matching event
        let matching = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.com/issuer")
            .with_subject("sub-42")
            .with_caller(Some("caller-nats".to_string()), Some("api_key".to_string()))
            .with_data(serde_json::json!({ "status": "issued" }));
        let matching_id = matching.id.clone();
        event_bus.publish(matching);

        let received = tokio::time::timeout(Duration::from_secs(3), received_receiver.recv())
            .await
            .expect("Mock NATS should receive published message")
            .expect("Channel should not be closed");

        let received_str = String::from_utf8_lossy(&received);
        assert!(received_str.contains("unicore.events"));
        assert!(received_str.contains(&matching_id));
        assert!(received_str.contains("caller-nats"));
        assert!(received_str.contains("com.impierce.unicore.credential.issued"));

        handle.abort();
        mock_server_handle.abort();
    }
}
