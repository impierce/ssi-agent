use agent_issuance::credential::{
    aggregate::Credential,
    event::CredentialEvent,
    views::all_credentials::AllCredentialsView,
};
use async_trait::async_trait;
use cqrs_es::{EventEnvelope, Query};
use oid4vci::notification_request::NotificationEvent;
use opentelemetry::metrics::Gauge;
use shared_kernel::view_repository::DynViewRepository;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, OnceLock},
};
use tracing::warn;

/// The name of the gauge holding the current number of credentials, excluding those reported as deleted.
pub const CREDENTIALS_COUNT_METRIC: &str = "credentials_count";

/// The view id under which the [`AllCredentialsView`] is stored (see `CqrsComponentBuilder`: `all_{Aggregate::TYPE}s`).
const ALL_CREDENTIALS_VIEW_ID: &str = "all_credentials";

/// Counts credentials based on the `Credential` events, excluding credentials that the holder has reported as
/// deleted (OID4VCI `credential_deleted` notification), and records the count as an OpenTelemetry gauge.
///
/// This projection is the blueprint for event-derived metrics; see `docs/metrics/README.md` for how to attach
/// and seed such a projection.
#[derive(Clone, Default)]
pub struct CredentialCountProjection {
    state: Arc<Mutex<CredentialCountState>>,
}

#[derive(Default)]
struct CredentialCountState {
    /// Credential ids that exist and have not been reported as deleted.
    live: HashSet<String>,
    /// Credential ids that the holder has reported as deleted. Kept so that late events for a deleted
    /// credential do not resurrect it in the count.
    deleted: HashSet<String>,
}

impl CredentialCountProjection {
    /// Initializes the count from the persisted `all_credentials` view, so that the metric reflects credentials
    /// issued before the current process started.
    pub async fn seed(&self, all_credentials: &Arc<dyn DynViewRepository<AllCredentialsView, Credential>>) {
        match all_credentials.load(ALL_CREDENTIALS_VIEW_ID).await {
            Ok(view) => {
                {
                    let mut state = self.state.lock().unwrap();
                    for (credential_id, credential) in view.unwrap_or_default().credentials {
                        if credential
                            .holder_notifications
                            .iter()
                            .any(|notification| notification.event == NotificationEvent::CredentialDeleted)
                        {
                            state.live.remove(&credential_id);
                            state.deleted.insert(credential_id);
                        } else if !state.deleted.contains(&credential_id) {
                            state.live.insert(credential_id);
                        }
                    }
                }
                self.record();
            }
            Err(error) => {
                warn!("Failed to load the `{ALL_CREDENTIALS_VIEW_ID}` view to seed the credential count metric: {error}");
            }
        }
    }

    /// The current number of credentials that have not been reported as deleted.
    #[cfg(test)]
    fn count(&self) -> usize {
        self.state.lock().unwrap().live.len()
    }

    /// Records the current count to the OpenTelemetry meter provider.
    ///
    /// The gauge is created lazily on the first record so that it is bound to the globally registered meter
    /// provider (a no-op provider when OpenTelemetry is not enabled): the projection is seeded after
    /// `init_telemetry`, and instruments created before the provider is registered would stay no-op forever.
    fn record(&self) {
        static GAUGE: OnceLock<Gauge<u64>> = OnceLock::new();

        let count = self.state.lock().unwrap().live.len() as u64;

        GAUGE
            .get_or_init(|| {
                opentelemetry::global::meter("unicore")
                    .u64_gauge(CREDENTIALS_COUNT_METRIC)
                    .with_description("The number of credentials, excluding those reported as deleted by the holder.")
                    .build()
            })
            .record(count, &[]);
    }
}

#[async_trait]
impl Query<Credential> for CredentialCountProjection {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Credential>]) {
        {
            let mut state = self.state.lock().unwrap();

            for event in events {
                match &event.payload {
                    CredentialEvent::NotificationReceived { notification, .. }
                        if notification.event == NotificationEvent::CredentialDeleted =>
                    {
                        state.live.remove(aggregate_id);
                        state.deleted.insert(aggregate_id.to_string());
                    }
                    // Any other event implies the credential exists.
                    _ => {
                        if !state.deleted.contains(aggregate_id) {
                            state.live.insert(aggregate_id.to_string());
                        }
                    }
                }
            }
        }
        self.record();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oid4vci::notification_request::NotificationRequest;
    use serde_json::json;
    use std::collections::HashMap;

    fn envelope(credential_id: &str, payload: CredentialEvent) -> EventEnvelope<Credential> {
        EventEnvelope {
            aggregate_id: credential_id.to_string(),
            sequence: 1,
            payload,
            metadata: HashMap::new(),
        }
    }

    fn created(credential_id: &str) -> EventEnvelope<Credential> {
        envelope(
            credential_id,
            CredentialEvent::SignedCredentialCreated {
                credential_id: credential_id.to_string(),
                signed_credential: json!({}),
                notification_id: None,
            },
        )
    }

    fn deleted(credential_id: &str) -> EventEnvelope<Credential> {
        envelope(
            credential_id,
            CredentialEvent::NotificationReceived {
                credential_id: credential_id.to_string(),
                notification: NotificationRequest {
                    notification_id: "notification-1".to_string(),
                    event: NotificationEvent::CredentialDeleted,
                    event_description: None,
                },
            },
        )
    }

    #[tokio::test]
    async fn counts_credentials_and_excludes_deleted_ones() {
        let projection = CredentialCountProjection::default();

        projection.dispatch("credential-1", &[created("credential-1")]).await;
        projection.dispatch("credential-2", &[created("credential-2")]).await;
        assert_eq!(projection.count(), 2);

        // Multiple events for the same credential must not count twice.
        projection.dispatch("credential-1", &[created("credential-1")]).await;
        assert_eq!(projection.count(), 2);

        projection.dispatch("credential-1", &[deleted("credential-1")]).await;
        assert_eq!(projection.count(), 1);

        // Late events for a deleted credential must not resurrect it in the count.
        projection.dispatch("credential-1", &[created("credential-1")]).await;
        assert_eq!(projection.count(), 1);
    }
}
