use crate::domain::event_publisher::config::{
    is_ip_allowed, validate_http_target_url, EventPublisherFilter, HttpPublisherConfig,
};
use reqwest::header::{HeaderValue, CONTENT_TYPE};
use shared_kernel::event_bus::{BusEventStream, EventBus, EventBusError, EventBusHandle};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio_stream::StreamExt;
use url::Url;

/// Default timeout for outgoing HTTP webhook requests.
pub const DEFAULT_WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

fn validate_resolved_addrs(addrs: &[SocketAddr], allow_local_network: bool) -> Result<(), String> {
    if addrs.is_empty() {
        return Err("HTTP webhook host did not resolve to any address".to_string());
    }
    for addr in addrs {
        if !is_ip_allowed(addr.ip(), allow_local_network) {
            return Err(format!(
                "resolved address {} is not permitted for HTTP webhook delivery",
                addr.ip()
            ));
        }
    }
    Ok(())
}

/// Custom `reqwest` DNS resolver that vets every resolved IP address against the outbound SSRF
/// policy and hands the vetted socket addresses directly to `hyper`, preventing DNS rebinding.
struct VettingDnsResolver {
    allow_local_network: bool,
}

impl reqwest::dns::Resolve for VettingDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allow_local_network = self.allow_local_network;
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            validate_resolved_addrs(&addrs, allow_local_network).map_err(|reason| {
                Box::new(std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            let iter: reqwest::dns::Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

/// Background worker that subscribes to the [`EventBusHandle`] for a single configured
/// HTTP event publisher and delivers matching [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s.
pub struct HttpEventPublisherWorker {
    id: String,
    client: reqwest::Client,
    target_url: String,
    target_host: Option<String>,
    allow_local_network: bool,
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
        Self::with_options(
            id,
            filter,
            config,
            event_bus,
            timeout,
            cfg!(feature = "allow-localhost"),
        )
    }

    /// Creates a new `HttpEventPublisherWorker` that permits local-network targets (`127.0.0.1`,
    /// `::1`, `localhost`, RFC 1918 / ULA) for wiremock-backed integration tests.
    ///
    /// # Panics
    ///
    /// Panics if the underlying TLS/HTTP client backend fails to initialize.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn new_allowing_local_network(
        id: String,
        filter: &EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
    ) -> Self {
        Self::with_options(id, filter, config, event_bus, DEFAULT_WEBHOOK_TIMEOUT, true)
    }

    pub(crate) fn with_options(
        id: String,
        filter: &EventPublisherFilter,
        config: HttpPublisherConfig,
        event_bus: &EventBusHandle,
        timeout: Duration,
        allow_local_network: bool,
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
            // A configured HTTP proxy would resolve the target independently and bypass DNS pinning.
            .no_proxy()
            .dns_resolver(Arc::new(VettingDnsResolver { allow_local_network }))
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
            allow_local_network,
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

            let target_host = self.target_host.as_deref().unwrap_or("<unknown>");
            match Url::parse(&self.target_url)
                .map_err(|err| err.to_string())
                .and_then(|parsed| validate_http_target_url(&parsed, self.allow_local_network))
            {
                Ok(()) => {}
                Err(reason) => {
                    tracing::warn!(
                        publisher_id = %self.id,
                        event_id = %cloud_event.id,
                        target_host,
                        reason = %reason,
                        "Blocked HTTP webhook delivery to disallowed target"
                    );
                    continue;
                }
            }

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
        let worker = HttpEventPublisherWorker::new_allowing_local_network(
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
        let worker = HttpEventPublisherWorker::with_options(
            "pub-err".to_string(),
            &EventPublisherFilter {
                types: vec!["test.error.event".to_string()],
            },
            config,
            &event_bus,
            Duration::from_millis(50),
            true,
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
        let worker = HttpEventPublisherWorker::new_allowing_local_network(
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

        let ok_handle = HttpEventPublisherWorker::new_allowing_local_network(
            "pub-ok".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("{}/hooks/cap-secret-200?sig=query-secret-200", mock_server.uri()),
                headers: None,
            },
            &event_bus,
        )
        .spawn();

        let warn_handle = HttpEventPublisherWorker::new_allowing_local_network(
            "pub-warn".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("{}/hooks/cap-secret-500?sig=query-secret-500", mock_server.uri()),
                headers: None,
            },
            &event_bus,
        )
        .spawn();

        let err_handle = HttpEventPublisherWorker::with_options(
            "pub-conn-err".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: "http://127.0.0.1:1/hooks/cap-secret-err?sig=query-secret-err".to_string(),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(100),
            true,
        )
        .spawn();

        let ipv6_err_handle = HttpEventPublisherWorker::with_options(
            "pub-ipv6-conn-err".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: "http://[::1]:1/hooks/ipv6-cap-secret?sig=ipv6-query-secret".to_string(),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(100),
            true,
        )
        .spawn();

        let blocked_handle = HttpEventPublisherWorker::with_options(
            "pub-ssrf-blocked".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: "http://169.254.169.254/latest/meta-data/blocked-cap-secret?sig=blocked-query-secret"
                    .to_string(),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(100),
            false,
        )
        .spawn();

        event_bus.publish(CloudEvent::new("test.log.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(200)).await;

        mock_server.verify().await;
        ok_handle.abort();
        warn_handle.abort();
        err_handle.abort();
        ipv6_err_handle.abort();
        blocked_handle.abort();

        for secret in [
            "cap-secret-200",
            "query-secret-200",
            "cap-secret-500",
            "query-secret-500",
            "cap-secret-err",
            "query-secret-err",
            "ipv6-cap-secret",
            "ipv6-query-secret",
            "blocked-cap-secret",
            "blocked-query-secret",
        ] {
            assert!(!logs_contain(secret), "leaked URL secret `{secret}` in logs");
        }
    }

    #[tokio::test]
    async fn http_worker_blocks_loopback_and_localhost_when_local_network_is_disallowed() {
        let mock_server = MockServer::start().await;
        let port = mock_server.address().port();

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(64);
        let filter = EventPublisherFilter {
            types: vec!["test.ssrf.event".to_string()],
        };

        let loopback_worker = HttpEventPublisherWorker::with_options(
            "pub-loopback".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("{}/webhook", mock_server.uri()),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(200),
            false,
        )
        .spawn();

        let localhost_worker = HttpEventPublisherWorker::with_options(
            "pub-localhost".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("http://localhost:{port}/webhook"),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(200),
            false,
        )
        .spawn();

        let userinfo_worker = HttpEventPublisherWorker::with_options(
            "pub-userinfo".to_string(),
            &filter,
            HttpPublisherConfig {
                target_url: format!("http://user:pass@127.0.0.1:{port}/webhook"),
                headers: None,
            },
            &event_bus,
            Duration::from_millis(200),
            true,
        )
        .spawn();

        event_bus.publish(CloudEvent::new("test.ssrf.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(200)).await;

        mock_server.verify().await;
        loopback_worker.abort();
        localhost_worker.abort();
        userinfo_worker.abort();
    }

    #[tokio::test]
    async fn vetting_dns_resolver_rejects_disallowed_and_mixed_addresses_and_pins_allowed_localhost() {
        use reqwest::dns::Resolve as _;
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        // 1. Unit checks on validate_resolved_addrs
        assert!(validate_resolved_addrs(&[], false).is_err());
        assert!(validate_resolved_addrs(&[], true).is_err());

        let public_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 0);
        let loopback_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        let private_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0);
        let metadata_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), 0);
        let cgnat_v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)), 0);
        let mapped_loopback = SocketAddr::new(IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()), 0);
        let mapped_metadata = SocketAddr::new(IpAddr::V6(Ipv4Addr::new(169, 254, 169, 254).to_ipv6_mapped()), 0);
        let siit_metadata = SocketAddr::new(IpAddr::V6("::ffff:0:169.254.169.254".parse().unwrap()), 0);
        let six_to_four_loopback = SocketAddr::new(IpAddr::V6("2002:7f00:1::".parse().unwrap()), 0);
        let link_local_v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)), 0);
        let rfc9637_before = SocketAddr::new(
            IpAddr::V6("3ffe:ffff:ffff:ffff:ffff:ffff:ffff:ffff".parse().unwrap()),
            0,
        );
        let rfc9637_start = SocketAddr::new(IpAddr::V6("3fff::".parse().unwrap()), 0);
        let rfc9637_mid = SocketAddr::new(IpAddr::V6("3fff::1".parse().unwrap()), 0);
        let rfc9637_end = SocketAddr::new(
            IpAddr::V6("3fff:0fff:ffff:ffff:ffff:ffff:ffff:ffff".parse().unwrap()),
            0,
        );
        let rfc9637_after = SocketAddr::new(IpAddr::V6("3fff:1000::".parse().unwrap()), 0);

        assert!(validate_resolved_addrs(&[public_v4], false).is_ok());
        assert!(validate_resolved_addrs(&[rfc9637_before], false).is_ok());
        assert!(validate_resolved_addrs(&[rfc9637_before], true).is_ok());
        assert!(validate_resolved_addrs(&[rfc9637_after], false).is_ok());
        assert!(validate_resolved_addrs(&[rfc9637_after], true).is_ok());
        // Mixed public + private/loopback/documentation must be rejected when local network is disallowed
        assert!(validate_resolved_addrs(&[public_v4, loopback_v4], false).is_err());
        assert!(validate_resolved_addrs(&[public_v4, private_v4], false).is_err());
        assert!(validate_resolved_addrs(&[public_v4, rfc9637_mid], false).is_err());
        assert!(validate_resolved_addrs(&[mapped_loopback], false).is_err());
        // Metadata, SIIT/6to4, RFC 9637 documentation, link-local, CGNAT stay blocked even with local network allowed
        assert!(validate_resolved_addrs(&[metadata_v4], true).is_err());
        assert!(validate_resolved_addrs(&[mapped_metadata], true).is_err());
        assert!(validate_resolved_addrs(&[siit_metadata], true).is_err());
        assert!(validate_resolved_addrs(&[six_to_four_loopback], true).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_start], false).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_start], true).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_mid], false).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_mid], true).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_end], false).is_err());
        assert!(validate_resolved_addrs(&[rfc9637_end], true).is_err());
        assert!(validate_resolved_addrs(&[link_local_v6], true).is_err());
        assert!(validate_resolved_addrs(&[cgnat_v4], true).is_err());
        // Loopback and private are permitted with local network opt-in
        assert!(validate_resolved_addrs(&[loopback_v4], true).is_ok());
        assert!(validate_resolved_addrs(&[private_v4], true).is_ok());

        // 2. Live DNS lookup for `localhost` via VettingDnsResolver
        let strict_resolver = VettingDnsResolver {
            allow_local_network: false,
        };
        assert!(strict_resolver.resolve("localhost".parse().unwrap()).await.is_err());

        let local_resolver = VettingDnsResolver {
            allow_local_network: true,
        };
        let resolved: Vec<SocketAddr> = local_resolver
            .resolve("localhost".parse().unwrap())
            .await
            .unwrap()
            .collect();
        assert_ne!(resolved.len(), 0);
        assert!(resolved.iter().all(|addr| addr.ip().is_loopback()));

        // 3. End-to-end delivery through VettingDnsResolver using hostname `localhost`
        let mock_server = MockServer::start().await;
        let port = mock_server.address().port();
        Mock::given(method("POST"))
            .and(path("/localhost-hook"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let event_bus = EventBusHandle::new(16);
        let worker = HttpEventPublisherWorker::new_allowing_local_network(
            "pub-localhost-ok".to_string(),
            &EventPublisherFilter {
                types: vec!["test.localhost.event".to_string()],
            },
            HttpPublisherConfig {
                target_url: format!("http://localhost:{port}/localhost-hook"),
                headers: None,
            },
            &event_bus,
        )
        .spawn();

        event_bus.publish(CloudEvent::new("test.localhost.event", "https://example.test/test"));
        tokio::time::sleep(Duration::from_millis(200)).await;

        mock_server.verify().await;
        worker.abort();
    }

    #[cfg(feature = "allow-localhost")]
    mod allow_localhost_tests {
        use super::*;

        #[tokio::test]
        async fn default_worker_delivers_to_loopback_when_allow_localhost_enabled() {
            let mock_server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/allow-localhost-hook"))
                .respond_with(ResponseTemplate::new(200))
                .expect(1)
                .mount(&mock_server)
                .await;

            let event_bus = EventBusHandle::new(16);
            let worker = HttpEventPublisherWorker::new(
                "pub-allow-localhost".to_string(),
                &EventPublisherFilter {
                    types: vec!["test.allow_localhost.event".to_string()],
                },
                HttpPublisherConfig {
                    target_url: format!("{}/allow-localhost-hook", mock_server.uri()),
                    headers: None,
                },
                &event_bus,
            )
            .spawn();

            event_bus.publish(CloudEvent::new(
                "test.allow_localhost.event",
                "https://example.test/test",
            ));
            tokio::time::sleep(Duration::from_millis(200)).await;

            mock_server.verify().await;
            worker.abort();
        }
    }
}
