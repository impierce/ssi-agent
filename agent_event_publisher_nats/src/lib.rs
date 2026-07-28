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
/// canonical [`CloudEvent`](shared_kernel::event_bus::CloudEvent)s to configured NATS subjects.
pub fn start_nats_forwarder(event_bus: EventBusHandle) -> Option<tokio::task::JoinHandle<()>> {
    let conf = config();
    let nats_config = conf.event_publishers.nats.as_ref()?;
    if !nats_config.enabled {
        return None;
    }

    let nats_url = nats_config.nats_url.clone();
    let subjects = nats_config.subjects.clone();

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
        let mut stream = event_bus.subscribe(EventFilter::default());

        while let Some(item) = stream.next().await {
            if let Ok(cloud_event) = item {
                for subject_config in &subjects {
                    if !matches_events_config(&subject_config.events, &cloud_event) {
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
                        tracing::error!("Failed to publish CloudEvent {:?} to NATS subject {}: {:?}", cloud_event.id, subject_name, err);
                    } else {
                        info!("Published CloudEvent {:?} to NATS subject {}", cloud_event.id, subject_name);
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
    async fn test_nats_forwarder_start() {
        let bus_handle = EventBusHandle::new(16);
        let _handle = start_nats_forwarder(bus_handle);
    }

    #[tokio::test]
    async fn test_cloud_event_serialization() {
        let event = build_cloud_event("offer", "123", 1, "Created", serde_json::json!({}), None);
        let bytes = serde_json::to_vec(&event).unwrap();
        assert!(!bytes.is_empty());
    }
}
