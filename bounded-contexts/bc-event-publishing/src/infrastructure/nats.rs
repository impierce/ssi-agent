use crate::domain::event_publisher::config::{EventPublisherFilter, NatsPublisherConfig};
use async_nats::Client;
use shared_kernel::event_bus::{BusEventStream, EventBus, EventBusError, EventBusHandle};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_stream::StreamExt;
use url::Url;

/// Background worker that subscribes to the [`EventBusHandle`] for a single configured
/// NATS event publisher and publishes matching [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s
/// to the configured NATS subject.
pub struct NatsEventPublisherWorker {
    id: String,
    client: Client,
    subject: String,
    stream: BusEventStream,
}

impl NatsEventPublisherWorker {
    /// Spawns a resilient NATS worker task that subscribes immediately to `event_bus` and
    /// connects to `config.nats_url` in the background (retrying with exponential backoff
    /// if the broker is temporarily unreachable) so runtime commands and startup hydration
    /// never block or fail when a NATS broker is restarting.
    #[must_use]
    pub fn spawn_resilient(
        id: String,
        filter: &EventPublisherFilter,
        config: NatsPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> tokio::task::JoinHandle<()> {
        Self::spawn_resilient_with_initial_backoff(id, filter, config, event_bus, Duration::from_secs(1))
    }

    fn build_connect_options(
        id: String,
        host: String,
        subject: String,
        disconnected: Arc<AtomicBool>,
    ) -> async_nats::ConnectOptions {
        async_nats::ConnectOptions::new().event_callback(move |event| {
            let cb_id = id.clone();
            let cb_host = host.clone();
            let cb_subject = subject.clone();
            let cb_disconnected = Arc::clone(&disconnected);
            async move {
                match event {
                    async_nats::Event::Disconnected => {
                        if !cb_disconnected.swap(true, Ordering::AcqRel) {
                            tracing::warn!(
                                publisher_id = %cb_id,
                                target_host = %cb_host,
                                subject = %cb_subject,
                                "NATS event publisher worker disconnected from broker; reconnecting"
                            );
                        }
                    }
                    async_nats::Event::Connected => {
                        if cb_disconnected.swap(false, Ordering::AcqRel) {
                            tracing::info!(
                                publisher_id = %cb_id,
                                target_host = %cb_host,
                                subject = %cb_subject,
                                "NATS event publisher worker reconnected to broker"
                            );
                        }
                    }
                    async_nats::Event::ClientError(err) => {
                        tracing::debug!(
                            publisher_id = %cb_id,
                            target_host = %cb_host,
                            error = %err,
                            "NATS event publisher background client error"
                        );
                    }
                    other => {
                        tracing::debug!(
                            publisher_id = %cb_id,
                            target_host = %cb_host,
                            event = %other,
                            "NATS event publisher client state update"
                        );
                    }
                }
            }
        })
    }

    fn spawn_resilient_with_initial_backoff(
        id: String,
        filter: &EventPublisherFilter,
        config: NatsPublisherConfig,
        event_bus: &EventBusHandle,
        initial_backoff: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let stream: BusEventStream = if filter.types.is_empty() {
            Box::pin(tokio_stream::empty())
        } else {
            event_bus.subscribe(filter.to_event_filter())
        };

        let candidate = if config.nats_url.contains("://") {
            config.nats_url.clone()
        } else {
            format!("nats://{}", config.nats_url)
        };
        let target_host = Url::parse(&candidate)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_owned));

        tokio::spawn(async move {
            let mut backoff = initial_backoff;
            let mut logged_connect_error = false;
            let host_owned = target_host.unwrap_or_else(|| "<unknown>".to_string());
            let disconnected = Arc::new(AtomicBool::new(false));

            let client = loop {
                let connect_options = Self::build_connect_options(
                    id.clone(),
                    host_owned.clone(),
                    config.subject.clone(),
                    Arc::clone(&disconnected),
                );

                match connect_options.connect(&config.nats_url).await {
                    Ok(client) => {
                        if logged_connect_error {
                            tracing::info!(
                                publisher_id = %id,
                                target_host = %host_owned,
                                subject = %config.subject,
                                "NATS event publisher worker connected after retry"
                            );
                        } else {
                            tracing::debug!(
                                publisher_id = %id,
                                target_host = %host_owned,
                                subject = %config.subject,
                                "NATS event publisher worker connected"
                            );
                        }
                        break client;
                    }
                    Err(err) => {
                        if logged_connect_error {
                            tracing::debug!(
                                publisher_id = %id,
                                target_host = %host_owned,
                                error = %err,
                                retry_in_ms = backoff.as_millis(),
                                "NATS event publisher worker still unable to connect; retrying"
                            );
                        } else {
                            logged_connect_error = true;
                            tracing::error!(
                                publisher_id = %id,
                                target_host = %host_owned,
                                error = %err,
                                retry_in_ms = backoff.as_millis(),
                                "Failed to connect NATS event publisher worker; retrying"
                            );
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(30));
                    }
                }
            };

            let worker = Self {
                id,
                client,
                subject: config.subject,
                stream,
            };
            worker.run().await;
        })
    }

    /// Runs the NATS event publisher loop until the stream closes or the task is aborted.
    async fn run(mut self) {
        while let Some(item) = self.stream.next().await {
            let cloud_event = match item {
                Ok(event) => event,
                Err(EventBusError::Closed) => break,
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
                        error = %err,
                        "NATS event publisher worker encountered event bus error"
                    );
                    continue;
                }
            };

            let payload = match serde_json::to_vec(&cloud_event) {
                Ok(p) => p,
                Err(err) => {
                    tracing::error!(
                        publisher_id = %self.id,
                        event_id = %cloud_event.id,
                        error = %err,
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
                    error = %err,
                    "Failed to publish CloudEvent to NATS subject"
                );
            } else {
                tracing::debug!(
                    publisher_id = %self.id,
                    event_id = %cloud_event.id,
                    subject = %self.subject,
                    "Published CloudEvent to NATS subject"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_kernel::event_bus::CloudEvent;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tracing_test::traced_test;

    #[tokio::test]
    #[traced_test]
    async fn nats_worker_unreachable_broker_logs_error_once_and_debug_on_subsequent_retries() {
        let event_bus = EventBusHandle::new(100);
        let config = NatsPublisherConfig {
            nats_url: "127.0.0.1:1".to_string(),
            subject: "test.subject".to_string(),
        };

        let handle = NatsEventPublisherWorker::spawn_resilient_with_initial_backoff(
            "nats-1".to_string(),
            &EventPublisherFilter {
                types: vec!["com.impierce.unicore.issuance.credential.offered".to_string()],
            },
            config,
            &event_bus,
            Duration::from_millis(10),
        );

        tokio::time::sleep(Duration::from_millis(70)).await;
        assert!(
            !handle.is_finished(),
            "Resilient worker should stay alive retrying when broker is unreachable"
        );
        handle.abort();

        assert!(logs_contain("Failed to connect NATS event publisher worker; retrying"));
        assert!(logs_contain(
            "NATS event publisher worker still unable to connect; retrying"
        ));
        logs_assert(|lines: &[&str]| {
            let error_count = lines.iter().filter(|line| line.contains("ERROR")).count();
            if error_count == 1 {
                Ok(())
            } else {
                Err(format!(
                    "expected exactly 1 ERROR log across all targets, got {error_count}: {lines:?}"
                ))
            }
        });
    }

    #[tokio::test]
    #[traced_test]
    async fn nats_worker_disconnect_and_reconnect_logs_state_transitions_without_error_spam() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let mock_server_handle = tokio::spawn(async move {
            let info = b"INFO {\"server_id\":\"TEST\",\"server_name\":\"test\",\"version\":\"2.10.0\",\"proto\":1,\"headers\":true,\"max_payload\":1048576}\r\n";

            // 1. Accept initial connection, respond to handshake PING, then drop socket to simulate broker crash.
            if let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(info).await;
                let mut buf = [0u8; 1024];
                if let Ok(n) = socket.read(&mut buf).await {
                    if buf[..n].windows(4).any(|w| w == b"PING") {
                        let _ = socket.write_all(b"PONG\r\n").await;
                    }
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
                drop(socket);
            }

            // 2. Accept reconnect and keep it alive briefly.
            if let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(info).await;
                let mut buf = [0u8; 1024];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if buf[..n].windows(4).any(|w| w == b"PING") {
                        let _ = socket.write_all(b"PONG\r\n").await;
                    }
                }
            }
        });

        let event_bus = EventBusHandle::new(100);
        let handle = NatsEventPublisherWorker::spawn_resilient(
            "nats-reconnect".to_string(),
            &EventPublisherFilter {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
            NatsPublisherConfig {
                nats_url: local_addr.to_string(),
                subject: "unicore.events".to_string(),
            },
            &event_bus,
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !logs_contain("NATS event publisher worker reconnected to broker") {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for NATS worker reconnect log"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        handle.abort();
        mock_server_handle.abort();

        assert!(logs_contain(
            "NATS event publisher worker disconnected from broker; reconnecting"
        ));
        assert!(logs_contain("NATS event publisher worker reconnected to broker"));
        logs_assert(|lines: &[&str]| {
            let error_count = lines.iter().filter(|line| line.contains("ERROR")).count();
            let warn_count = lines
                .iter()
                .filter(|line| {
                    line.contains("WARN")
                        && line.contains("NATS event publisher worker disconnected from broker; reconnecting")
                })
                .count();
            if error_count == 0 && warn_count == 1 {
                Ok(())
            } else {
                Err(format!(
                    "expected 0 ERROR logs and 1 disconnect WARN log, got {error_count} ERROR and {warn_count} WARN: {lines:?}"
                ))
            }
        });
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
        let handle = NatsEventPublisherWorker::spawn_resilient(
            "nats-1".to_string(),
            &EventPublisherFilter {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
            config,
            &event_bus,
        );

        tokio::time::sleep(Duration::from_millis(150)).await;

        // 1. Non-matching event
        event_bus.publish(CloudEvent::new("org.other.event", "https://example.test/other"));

        // 2. Matching event
        let matching = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.test/issuer")
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
