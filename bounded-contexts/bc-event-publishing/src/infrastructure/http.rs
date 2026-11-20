use crate::domain::event_publisher::config::{EventPublisherFilter, HttpPublisherConfig};
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use shared_kernel::event_bus::{BusEventStream, EventBus, EventBusError, EventBusHandle};
use std::time::Duration;
use tokio_stream::StreamExt;
use url::Url;

/// Default timeout for outgoing HTTP webhook requests.
pub const DEFAULT_WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Background worker that subscribes to the [`EventBusHandle`] for a single configured
/// HTTP event publisher and delivers matching [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s.
pub struct HttpEventPublisherWorker {
    id: String,
    client: reqwest::Client,
    target_url: String,
    target_host: Option<String>,
    stream: BusEventStream,
}

impl HttpEventPublisherWorker {
    /// Creates a new `HttpEventPublisherWorker` with the default request timeout (10s).
    ///
    /// Subscribes immediately to `event_bus` upon creation so no events published after
    /// this call are missed.
    ///
    /// # Panics
    ///
    /// Panics if the underlying TLS/HTTP client backend fails to initialize.
    #[must_use]
    pub fn new(
        id: String,
        filter: &EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> Self {
        Self::with_timeout(id, filter, config, event_bus, DEFAULT_WEBHOOK_TIMEOUT)
    }

    /// Creates a new `HttpEventPublisherWorker` with a custom request timeout.
    ///
    /// # Panics
    ///
    /// Panics if the underlying TLS/HTTP client backend fails to initialize.
    #[must_use]
    pub fn with_timeout(
        id: String,
        filter: &EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
        timeout: Duration,
    ) -> Self {
        let stream: BusEventStream = if filter.types.is_empty() {
            Box::pin(tokio_stream::empty())
        } else {
            event_bus.subscribe(filter.to_event_filter())
        };

        let mut default_headers = config.header_map();
        // CloudEvents JSON format envelope media type:
        // https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/formats/json-format.md#3-envelope
        // Inserted after custom headers so publisher headers cannot override Content-Type.
        default_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/cloudevents+json"));

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(default_headers)
            .timeout(timeout)
            .build()
            .expect("failed to build HTTP event publisher reqwest::Client");

        let target_host = Url::parse(&config.target_url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_owned));

        Self {
            id,
            client,
            target_url: config.target_url,
            target_host,
            stream,
        }
    }

    /// Runs the HTTP event publisher delivery loop until the stream closes or the task is aborted.
    pub async fn run(mut self) {
        while let Some(item) = self.stream.next().await {
            let cloud_event = match item {
                Ok(event) => event,
                Err(EventBusError::Closed) => break,
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
                        error = %err,
                        "HTTP event publisher worker encountered event bus error"
                    );
                    continue;
                }
            };

            let payload = match serde_json::to_vec(&cloud_event) {
                Ok(payload) => payload,
                Err(err) => {
                    tracing::error!(
                        publisher_id = %self.id,
                        event_id = %cloud_event.id,
                        error = %err,
                        "Failed to serialize CloudEvent for HTTP webhook target"
                    );
                    continue;
                }
            };

            let target_host = self.target_host.as_deref().unwrap_or("<unknown>");
            match self.client.post(&self.target_url).body(payload).send().await {
                Ok(res) => {
                    if res.status().is_success() {
                        tracing::debug!(
                            publisher_id = %self.id,
                            event_id = %cloud_event.id,
                            target_host,
                            "Successfully forwarded CloudEvent to HTTP webhook target"
                        );
                    } else {
                        tracing::warn!(
                            publisher_id = %self.id,
                            event_id = %cloud_event.id,
                            target_host,
                            status = %res.status(),
                            "HTTP webhook target returned non-success status"
                        );
                    }
                }
                Err(err) => {
                    let sanitized_err = err.without_url();
                    tracing::error!(
                        publisher_id = %self.id,
                        event_id = %cloud_event.id,
                        target_host,
                        error = %sanitized_err,
                        "Failed to send CloudEvent to HTTP webhook target"
                    );
                }
            }
        }
    }

    /// Spawns the HTTP event publisher worker as a background task on the Tokio runtime.
    #[must_use]
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_kernel::event_bus::CloudEvent;
    use std::collections::BTreeMap;
    use tracing_test::traced_test;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn http_worker_publishes_matching_events_with_cloudevents_content_type_and_headers() {
        let mock_server = MockServer::start().await;

        let mut headers = BTreeMap::new();
        headers.insert("x-webhook-secret".to_string(), "supersecret".to_string());
        // Even if constructed directly in Rust with a conflicting content-type header,
        // the worker must enforce `application/cloudevents+json`.
        headers.insert("content-type".to_string(), "text/plain".to_string());

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
            &EventPublisherFilter {
                types: vec!["com.impierce.unicore.credential.issued".to_string()],
            },
            config,
            &event_bus,
        );
        let handle = worker.spawn();

        // 1. Non-matching event -> not forwarded
        let non_matching = CloudEvent::new("com.other.domain.event", "https://example.test/other");
        event_bus.publish(non_matching);

        // 2. Matching event -> forwarded
        let matching = CloudEvent::new("com.impierce.unicore.credential.issued", "https://example.test/issuer")
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
            &EventPublisherFilter {
                types: vec!["test.error.event".to_string()],
            },
            config,
            &event_bus,
            Duration::from_millis(50),
        );
        let handle = worker.spawn();

        event_bus.publish(CloudEvent::new("test.error.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(120)).await;

        assert!(!handle.is_finished());
        handle.abort();
    }

    #[tokio::test]
    async fn http_worker_does_not_follow_redirects() {
        let mock_server = MockServer::start().await;
        let redirected_target = format!("{}/redirected", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(307).insert_header("Location", redirected_target.as_str()))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/redirected"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let config = HttpPublisherConfig {
            target_url: format!("{}/webhook", mock_server.uri()),
            headers: None,
        };

        let event_bus = EventBusHandle::new(100);
        let worker = HttpEventPublisherWorker::new(
            "pub-redirect".to_string(),
            &EventPublisherFilter {
                types: vec!["test.redirect.event".to_string()],
            },
            config,
            &event_bus,
        );
        let handle = worker.spawn();

        event_bus.publish(CloudEvent::new("test.redirect.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(150)).await;

        mock_server.verify().await;
        handle.abort();
    }

    #[tokio::test]
    #[traced_test]
    async fn http_worker_logs_never_leak_capability_paths_or_query_secrets() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/hooks/cap-secret-200"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/hooks/cap-secret-500"))
            .respond_with(ResponseTemplate::new(502))
            .expect(1)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(100);
        let filter = EventPublisherFilter {
            types: vec!["test.log.event".to_string()],
        };

        let ok_handle = HttpEventPublisherWorker::new(
            "pub-ok".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("{}/hooks/cap-secret-200?sig=query-secret-200", mock_server.uri()),
                headers: None,
            },
            &event_bus,
        )
        .spawn();

        let warn_handle = HttpEventPublisherWorker::new(
            "pub-warn".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("{}/hooks/cap-secret-500?sig=query-secret-500", mock_server.uri()),
                headers: None,
            },
            &event_bus,
        )
        .spawn();

        let err_handle = HttpEventPublisherWorker::with_timeout(
            "pub-conn-err".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: "http://127.0.0.1:1/hooks/cap-secret-err?sig=query-secret-err".to_string(),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(100),
        )
        .spawn();

        let ipv6_err_handle = HttpEventPublisherWorker::with_timeout(
            "pub-ipv6-conn-err".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: "http://[::1]:1/hooks/ipv6-cap-secret?sig=ipv6-query-secret".to_string(),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(100),
        )
        .spawn();

        event_bus.publish(CloudEvent::new("test.log.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(200)).await;

        mock_server.verify().await;
        ok_handle.abort();
        warn_handle.abort();
        err_handle.abort();
        ipv6_err_handle.abort();

        for secret in [
            "cap-secret-200",
            "query-secret-200",
            "cap-secret-500",
            "query-secret-500",
            "cap-secret-err",
            "query-secret-err",
            "ipv6-cap-secret",
            "ipv6-query-secret",
        ] {
            assert!(!logs_contain(secret), "leaked URL secret `{secret}` in logs");
        }
    }
}
