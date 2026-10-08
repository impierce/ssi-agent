use std::time::Duration;

use agent_shared::config::{config, EventPublisherHttp};
use shared_kernel::event_bus::{BusEventStream, CloudEvent, EventBus, EventBusError, EventBusHandle, EventFilter};
use tokio_stream::StreamExt;
use tracing::info;

/// Default timeout for outgoing HTTP webhook requests.
const DEFAULT_WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Background event publisher that subscribes to the [`EventBusHandle`] and publishes
/// canonical [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s to configured HTTP webhook endpoints.
pub struct HttpEventPublisher {
    client: reqwest::Client,
    configs: Vec<EventPublisherHttp>,
    stream: BusEventStream,
    timeout: Duration,
}

impl HttpEventPublisher {
    /// Creates a new `HttpEventPublisher` with explicitly provided configurations and default timeout.
    ///
    /// Subscribes immediately to `event_bus` upon creation to prevent event loss.
    /// Returns `None` if `configs` contains no enabled endpoints.
    pub fn new(configs: Vec<EventPublisherHttp>, event_bus: &EventBusHandle) -> Option<Self> {
        Self::with_timeout(configs, event_bus, DEFAULT_WEBHOOK_TIMEOUT)
    }

    /// Creates a new `HttpEventPublisher` with custom request timeout.
    pub fn with_timeout(
        configs: Vec<EventPublisherHttp>,
        event_bus: &EventBusHandle,
        timeout: Duration,
    ) -> Option<Self> {
        let enabled_configs: Vec<_> = configs.into_iter().filter(|c| c.enabled).collect();
        if enabled_configs.is_empty() {
            return None;
        }

        for target in &enabled_configs {
            if target.events.types.is_empty() {
                tracing::warn!(
                    "HTTP webhook target '{}' has no event types configured in 'events.types'; no events will be forwarded to this endpoint.",
                    target.target_url
                );
            }
        }

        let stream = event_bus.subscribe(EventFilter::default());

        Some(Self {
            client: reqwest::Client::new(),
            configs: enabled_configs,
            stream,
            timeout,
        })
    }

    /// Creates an `HttpEventPublisher` populated from the global application configuration.
    pub fn from_config(event_bus: &EventBusHandle) -> Option<Self> {
        Self::new(config().event_publishers.http.clone(), event_bus)
    }

    /// Runs the HTTP event publisher loop.
    pub async fn run(mut self) {
        info!("Starting HTTP event publisher for {} endpoints...", self.configs.len());

        while let Some(item) = self.stream.next().await {
            let cloud_event = match item {
                Ok(event) => event,
                // See docs/adr/0008-event-publisher-delivery-guarantees-and-lag-handling.md
                Err(EventBusError::Lagged(dropped_count)) => {
                    tracing::warn!(
                        "HTTP event publisher lagged behind by {} events; events were dropped",
                        dropped_count
                    );
                    continue;
                }
                Err(err) => {
                    tracing::error!("HTTP event publisher encountered event bus error: {:?}", err);
                    continue;
                }
            };

            for target_config in &self.configs {
                if target_config.events.types.is_empty() {
                    continue;
                }

                let filter = EventFilter {
                    event_types: target_config.events.types.clone(),
                    ..Default::default()
                };
                if !filter.matches(&cloud_event) {
                    continue;
                }

                tokio::spawn(forward(
                    self.request(target_config, &cloud_event),
                    cloud_event.id.clone(),
                    target_config.target_url.clone(),
                ));
            }
        }
    }

    /// Builds the webhook request for `cloud_event` against `target`, with the configured headers and timeout.
    fn request(&self, target: &EventPublisherHttp, cloud_event: &CloudEvent) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&target.target_url)
            // CloudEvents JSON format envelope media type:
            // https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/formats/json-format.md#3-envelope
            .header(reqwest::header::CONTENT_TYPE, "application/cloudevents+json");

        if let Some(headers) = &target.headers {
            req = req.headers(headers.clone());
        }

        req.json(cloud_event).timeout(self.timeout)
    }

    /// Spawns the HTTP event publisher as a background task on the Tokio runtime.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }
}

/// Sends one webhook request and logs the outcome.
///
/// Returns the response status, or the transport error (which includes request timeouts).
async fn forward(
    req: reqwest::RequestBuilder,
    event_id: String,
    target_url: String,
) -> Result<reqwest::StatusCode, reqwest::Error> {
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            if status.is_success() {
                info!(
                    "Successfully forwarded CloudEvent {:?} to HTTP webhook target {}",
                    event_id, target_url
                );
            } else {
                tracing::warn!("HTTP webhook target {} returned status {}", target_url, status);
            }
            Ok(status)
        }
        Err(err) => {
            tracing::error!(
                "Failed to send CloudEvent {:?} to HTTP webhook target {}: {:?}",
                event_id,
                target_url,
                err
            );
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_shared::config::{set_config, EventPublisherHttp, Events};
    use reqwest::header::HeaderMap;
    use tokio::sync::Mutex;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    static TEST_MUTEX: Mutex<()> = Mutex::const_new(());

    /// Restores the global HTTP publisher configuration when dropped, so a failing test cannot leak its
    /// configuration into the next one (ADR 0003 §2).
    struct RestoreHttpConfig(Vec<EventPublisherHttp>);

    impl Drop for RestoreHttpConfig {
        fn drop(&mut self) {
            set_config().event_publishers.http = std::mem::take(&mut self.0);
        }
    }

    #[tokio::test]
    async fn test_http_event_publisher_new_disabled() {
        let event_bus = EventBusHandle::new(100);

        // Empty configs -> None
        assert!(HttpEventPublisher::new(vec![], &event_bus).is_none());

        // Only disabled configs -> None
        let disabled_configs = vec![EventPublisherHttp {
            enabled: false,
            target_url: "http://localhost:12345/webhook".to_string(),
            headers: None,
            events: Events {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
        }];
        assert!(HttpEventPublisher::new(disabled_configs, &event_bus).is_none());
    }

    #[tokio::test]
    async fn test_http_event_publisher_publishes_matching_events() {
        let mock_server = MockServer::start().await;

        let mut headers = HeaderMap::new();
        headers.insert("x-webhook-secret", "supersecret".parse().unwrap());
        headers.insert(
            "x-custom-bytes",
            reqwest::header::HeaderValue::from_bytes(b"non-ascii-\xff").unwrap(),
        );

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .and(header("x-webhook-secret", "supersecret"))
            .and(header("content-type", "application/cloudevents+json"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let configs = vec![EventPublisherHttp {
            enabled: true,
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: Some(headers),
            events: Events {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
        }];

        let event_bus = EventBusHandle::new(100);
        let publisher = HttpEventPublisher::new(configs, &event_bus).expect("Expected publisher instance when enabled");
        let handle = publisher.spawn();

        // 1. Publish non-matching event -> should NOT be forwarded to mock server
        let non_matching_event = CloudEvent::new("com.other.domain.event", "https://example.com/other");
        event_bus.publish(non_matching_event);

        // 2. Publish matching event -> should be forwarded to mock server
        let matching_event = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.com/issuer")
            .with_subject("subject-42")
            .with_caller(Some("caller-abc".to_string()), Some("api_key".to_string()))
            .with_data(serde_json::json!({ "credential_id": "cred-99" }));

        let matching_event_id = matching_event.id.clone();
        event_bus.publish(matching_event);

        // Wait briefly for the spawned forwarder task to deliver the request
        tokio::time::sleep(Duration::from_millis(300)).await;

        mock_server.verify().await;

        let received_requests = mock_server.received_requests().await.unwrap();
        assert_eq!(received_requests.len(), 1);

        let received_event: CloudEvent =
            serde_json::from_slice(&received_requests[0].body).expect("Valid CloudEvent JSON");
        assert_eq!(received_event.id, matching_event_id);
        assert_eq!(received_event.event_type, "com.impierce.unicore.credential.issued");
        assert_eq!(received_event.source, "https://example.com/issuer");
        assert_eq!(received_event.subject, Some("subject-42".to_string()));
        assert_eq!(received_event.extension.callerid, Some("caller-abc".to_string()));
        assert_eq!(received_event.extension.callertype, Some("api_key".to_string()));
        assert_eq!(
            received_requests[0].headers.get("x-custom-bytes").unwrap().as_bytes(),
            b"non-ascii-\xff"
        );
        assert_eq!(
            received_event.data,
            Some(serde_json::json!({ "credential_id": "cred-99" }))
        );

        handle.abort();
    }

    #[tokio::test]
    async fn test_http_event_publisher_handles_endpoint_error_gracefully() {
        let mock_server = MockServer::start().await;

        // Mock returns 500 Internal Server Error
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&mock_server)
            .await;

        let configs = vec![EventPublisherHttp {
            enabled: true,
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: None,
            events: Events {
                types: vec!["test.error.event".to_string()],
            },
        }];

        let event_bus = EventBusHandle::new(100);
        let publisher = HttpEventPublisher::new(configs, &event_bus).expect("Expected publisher instance when enabled");
        let handle = publisher.spawn();

        let event = CloudEvent::new("test.error.event", "https://example.com/test");
        event_bus.publish(event);

        tokio::time::sleep(Duration::from_millis(300)).await;

        mock_server.verify().await;
        // Verify task is still alive despite the 500 response
        assert!(!handle.is_finished());

        handle.abort();
    }

    #[tokio::test]
    async fn test_http_event_publisher_empty_types_does_not_forward() {
        let mock_server = MockServer::start().await;

        // Expect 0 calls because types is empty
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let configs = vec![EventPublisherHttp {
            enabled: true,
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: None,
            events: Events { types: vec![] },
        }];

        let event_bus = EventBusHandle::new(100);
        let publisher = HttpEventPublisher::new(configs, &event_bus).expect("Expected publisher instance when enabled");
        let handle = publisher.spawn();

        let event = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.com/issuer");
        event_bus.publish(event);

        tokio::time::sleep(Duration::from_millis(200)).await;

        mock_server.verify().await;

        handle.abort();
    }

    #[tokio::test]
    async fn test_http_event_publisher_from_config() {
        let _guard = TEST_MUTEX.lock().await;
        let _restore = RestoreHttpConfig(std::mem::take(&mut set_config().event_publishers.http));

        let event_bus = EventBusHandle::new(100);
        assert!(HttpEventPublisher::from_config(&event_bus).is_none());

        set_config().event_publishers.http = vec![EventPublisherHttp {
            enabled: true,
            target_url: "http://localhost:12345/webhook".to_string(),
            headers: None,
            events: Events {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
        }];

        let publisher = HttpEventPublisher::from_config(&event_bus);
        assert!(publisher.is_some());
        if let Some(p) = publisher {
            let h = p.spawn();
            h.abort();
        }
    }

    #[tokio::test]
    async fn test_http_event_publisher_times_out_on_slow_endpoint() {
        let mock_server = MockServer::start().await;

        // The endpoint answers only after 200ms, well beyond the publisher's timeout.
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(200)))
            .expect(1)
            .mount(&mock_server)
            .await;

        let target_url = format!("{}/webhook", mock_server.uri());
        let configs = vec![EventPublisherHttp {
            enabled: true,
            target_url: target_url.clone(),
            headers: None,
            events: Events {
                types: vec!["test.timeout.event".to_string()],
            },
        }];

        let event_bus = EventBusHandle::new(100);
        let publisher = HttpEventPublisher::with_timeout(configs, &event_bus, Duration::from_millis(50))
            .expect("Expected publisher instance when enabled");

        // Send the request the publisher would spawn, so the outcome of the timeout is observable.
        let event = CloudEvent::new("test.timeout.event", "https://example.com/test");
        let request = publisher.request(&publisher.configs[0], &event);
        let err = forward(request, event.id.clone(), target_url)
            .await
            .expect_err("Expected the request to be abandoned before the endpoint responds");
        assert!(err.is_timeout(), "Expected a timeout error, got: {err:?}");

        // The request did reach the endpoint; the publisher gave up waiting for the response.
        mock_server.verify().await;
    }
}
