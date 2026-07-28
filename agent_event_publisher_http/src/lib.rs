use agent_shared::config::config;
use shared_kernel::event_bus::{EventBus, EventBusHandle, EventFilter};
use tokio_stream::StreamExt;
use tracing::info;

fn matches_events_config(events: &agent_shared::config::Events, cloud_event: &shared_kernel::event_bus::CloudEvent) -> bool {
    let type_clean = cloud_event.event_type.to_lowercase().replace('_', "");
    let mut any_filter_set = false;

    macro_rules! check_events {
        ($list:expr) => {
            if !$list.is_empty() {
                any_filter_set = true;
                if $list.iter().any(|e| {
                    let target_clean = e.to_string().to_lowercase().replace('_', "");
                    type_clean.contains(&target_clean)
                }) {
                    return true;
                }
            }
        };
    }

    check_events!(events.access_token);
    check_events!(events.authorization_code);
    check_events!(events.client);
    check_events!(events.oauth2_authorization_request);
    check_events!(events.connection);
    check_events!(events.document);
    check_events!(events.profile);
    check_events!(events.service);
    check_events!(events.template);
    check_events!(events.server_config);
    check_events!(events.credential);
    check_events!(events.offer);
    check_events!(events.nonce);
    check_events!(events.status_list);
    check_events!(events.holder_credential);
    check_events!(events.presentation);
    check_events!(events.received_offer);
    check_events!(events.authorization_request);

    !any_filter_set
}

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
        info!("Starting HTTP webhook event publisher forwarder for {} endpoints...", http_configs.len());
        let client = reqwest::Client::new();
        let mut stream = event_bus.subscribe(EventFilter::default());

        while let Some(item) = stream.next().await {
            if let Ok(cloud_event) = item {
                for target_config in &http_configs {
                    if !matches_events_config(&target_config.events, &cloud_event) {
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use shared_kernel::event_bus::build_cloud_event;

    #[tokio::test]
    async fn test_http_forwarder_start() {
        let bus_handle = EventBusHandle::new(16);
        let _handle = start_http_forwarder(bus_handle);
    }

    #[tokio::test]
    async fn test_cloud_event_json_formatting() {
        let event = build_cloud_event("credential", "cred-123", 1, "Signed", serde_json::json!({"ok": true}), None);
        let json_val = serde_json::to_value(&event).unwrap();
        assert_eq!(json_val["subject"], "cred-123");
    }
}
