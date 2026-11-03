use crate::domain::event_publisher::config::{EventPublisherFilter, HttpPublisherConfig};
use reqwest::header::HeaderMap;
use shared_kernel::event_bus::{BusEventStream, EventBus, EventBusError, EventBusHandle, EventFilter};
use std::time::Duration;
use tokio_stream::StreamExt;
use tracing::info;

/// Default timeout for outgoing HTTP webhook requests.
pub const DEFAULT_WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Background worker that subscribes to the [`EventBusHandle`] for a single configured
/// HTTP event publisher and delivers matching [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s.
pub struct HttpEventPublisherWorker {
    id: String,
    client: reqwest::Client,
    target_url: String,
    headers: HeaderMap,
    filter: EventFilter,
    stream: BusEventStream,
    timeout: Duration,
}

impl HttpEventPublisherWorker {
    /// Creates a new `HttpEventPublisherWorker` with the default request timeout (10s).
    ///
    /// Subscribes immediately to `event_bus` upon creation so no events published after
    /// this call are missed.
    #[must_use]
    pub fn new(
        id: String,
        filter: EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> Self {
        Self::with_timeout(id, filter, config, event_bus, DEFAULT_WEBHOOK_TIMEOUT)
    }

    /// Creates a new `HttpEventPublisherWorker` with a custom request timeout.
    #[must_use]
    pub fn with_timeout(
        id: String,
        filter: EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
        timeout: Duration,
    ) -> Self {
        let filter = filter.to_event_filter();
        let stream = event_bus.subscribe(filter.clone());
        let headers = config.header_map();

        Self {
            id,
            client: reqwest::Client::new(),
            target_url: config.target_url,
            headers,
            filter,
            stream,
            timeout,
        }
    }

    /// Runs the HTTP event publisher delivery loop until the stream closes or the task is aborted.
    pub async fn run(mut self) {
        info!(
            publisher_id = %self.id,
            target_url = %self.target_url,
            "Starting HTTP event publisher worker"
        );

        while let Some(item) = self.stream.next().await {
            let cloud_event = match item {
                Ok(event) => event,
                Err(EventBusError::Lagged(dropped_count)) => {
                    tracing::warn!(
                        publisher_id = %self.id,
                        dropped_count,
                        "HTTP event publisher worker lagged behind; events were dropped"
                    );
                    continue;
                }
                Err(err) => {
                    tracing::error!(
                        publisher_id = %self.id,
                        error = ?err,
                        "HTTP event publisher worker encountered event bus error"
                    );
                    continue;
                }
            };

            if self.filter.event_types.is_empty() || !self.filter.matches(&cloud_event) {
                continue;
            }

            let mut req = self
                .client
                .post(&self.target_url)
                // CloudEvents JSON format envelope media type:
                // https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/formats/json-format.md#3-envelope
                .header(reqwest::header::CONTENT_TYPE, "application/cloudevents+json");

            if !self.headers.is_empty() {
                req = req.headers(self.headers.clone());
            }

            let req = req.json(&cloud_event).timeout(self.timeout);
            let publisher_id = self.id.clone();
            let event_id = cloud_event.id.clone();
            let target_url = self.target_url.clone();

            tokio::spawn(async move {
                match req.send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            info!(
                                publisher_id = %publisher_id,
                                event_id = %event_id,
                                target_url = %target_url,
                                "Successfully forwarded CloudEvent to HTTP webhook target"
                            );
                        } else {
                            tracing::warn!(
                                publisher_id = %publisher_id,
                                event_id = %event_id,
                                target_url = %target_url,
                                status = %res.status(),
                                "HTTP webhook target returned non-success status"
                            );
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            publisher_id = %publisher_id,
                            event_id = %event_id,
                            target_url = %target_url,
                            error = ?err,
                            "Failed to send CloudEvent to HTTP webhook target"
                        );
                    }
                }
            });
        }
    }

    /// Spawns the HTTP event publisher worker as a background task on the Tokio runtime.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_kernel::event_bus::CloudEvent;
    use std::collections::BTreeMap;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn http_worker_publishes_matching_events_with_headers() {
        let mock_server = MockServer::start().await;

        let mut headers = BTreeMap::new();
        headers.insert("x-webhook-secret".to_string(), "supersecret".to_string());

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .and(header("x-webhook-secret", "supersecret"))
            .and(header("content-type", "application/cloudevents+json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = HttpPublisherConfig {
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: Some(headers),
        };

        let event_bus = EventBusHandle::new(100);
        let worker = HttpEventPublisherWorker::new(
            "pub-1".to_string(),
            EventPublisherFilter {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
            config,
            &event_bus,
        );
        let handle = worker.spawn();

        // 1. Non-matching event -> not forwarded
        let non_matching = CloudEvent::new("com.other.domain.event", "https://example.com/other");
        event_bus.publish(non_matching);

        // 2. Matching event -> forwarded
        let matching = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.com/issuer")
            .with_subject("subject-42")
            .with_caller(Some("caller-abc".to_string()), Some("api_key".to_string()))
            .with_data(serde_json::json!({ "credential_id": "cred-99" }));
        let matching_id = matching.id.clone();
        event_bus.publish(matching);

        tokio::time::sleep(Duration::from_millis(250)).await;
        mock_server.verify().await;

        let received = mock_server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let received_event: CloudEvent = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(received_event.id, matching_id);
        assert_eq!(received_event.subject.as_deref(), Some("subject-42"));
        assert_eq!(received_event.extension.callerid.as_deref(), Some("caller-abc"));

        handle.abort();
    }

    #[tokio::test]
    async fn http_worker_handles_endpoint_error_and_timeout_gracefully() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500).set_delay(Duration::from_millis(150)))
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = HttpPublisherConfig {
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: None,
        };

        let event_bus = EventBusHandle::new(100);
        let worker = HttpEventPublisherWorker::with_timeout(
            "pub-err".to_string(),
            EventPublisherFilter {
                types: vec!["test.error.event".to_string()],
            },
            config,
            &event_bus,
            Duration::from_millis(50),
        );
        let handle = worker.spawn();

        event_bus.publish(CloudEvent::new("test.error.event", "https://example.com/test"));
        tokio::time::sleep(Duration::from_millis(120)).await;

        assert!(!handle.is_finished());
        handle.abort();
    }
}
