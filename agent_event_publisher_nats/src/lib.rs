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
