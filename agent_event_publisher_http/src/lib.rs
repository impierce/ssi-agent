use agent_shared::config::config;
use shared_kernel::event_bus::{EventBus, EventBusHandle, EventFilter};
use tokio_stream::StreamExt;
use tracing::info;

/// Spawns a background worker that subscribes to the [`EventBusHandle`] and forwards
/// canonical [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s to configured HTTP webhook endpoints.
pub fn start_http_forwarder(event_bus: EventBusHandle) -> Option<tokio::task::JoinHandle<()>> {
    let http_configs: Vec<_> = config()
        .event_publishers
        .http
        .iter()
        .filter(|c| c.enabled)
        .cloned()
        .collect();

    if http_configs.is_empty() {
        return None;
    }

    Some(tokio::spawn(async move {
        info!(
            "Starting HTTP webhook event publisher forwarder for {} endpoints...",
            http_configs.len()
        );
        let client = reqwest::Client::new();
        let mut stream = event_bus.subscribe(EventFilter::default());

        while let Some(item) = stream.next().await {
            if let Ok(cloud_event) = item {
                for target_config in &http_configs {
                    let filter = EventFilter {
                        event_types: target_config.events.types.clone(),
                        ..Default::default()
                    };
                    if !filter.matches(&cloud_event) {
                        continue;
                    }

                    let mut req = client.post(&target_config.target_url);

                    if let Some(headers) = &target_config.headers {
                        for (header_name, header_value) in headers {
                            req = req.header(header_name.as_str(), header_value.to_str().unwrap_or(""));
                        }
                    }

                    let req = req.json(&cloud_event);

                    match req.send().await {
                        Ok(res) => {
                            if res.status().is_success() {
                                info!(
                                    "Successfully forwarded CloudEvent {:?} to HTTP webhook target {}",
                                    cloud_event.id, target_config.target_url
                                );
                            } else {
                                tracing::warn!(
                                    "HTTP webhook target {} returned status {}",
                                    target_config.target_url,
                                    res.status()
                                );
                            }
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to send CloudEvent {:?} to HTTP webhook target {}: {:?}",
                                cloud_event.id,
                                target_config.target_url,
                                err
                            );
                        }
                    }
                }
            }
        }
    }))
}
