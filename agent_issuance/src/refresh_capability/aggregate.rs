use std::sync::Arc;

use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{
    refresh_capability::{
        command::RefreshCapabilityCommand,
        error::RefreshCapabilityError,
        event::RefreshCapabilityEvent::{self, RefreshCapabilityCreated, RefreshCapabilityDisabled},
    },
    services::IssuanceServices,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct RefreshCapability {
    #[serde(rename = "id")]
    pub refresh_reference: String,
    pub credential_id: String,
    pub status: RefreshCapabilityStatus,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshCapabilityStatus {
    #[default]
    Active,
    Disabled,
}

impl Aggregate for RefreshCapability {
    type Command = RefreshCapabilityCommand;
    type Event = RefreshCapabilityEvent;
    type Error = RefreshCapabilityError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "refresh_capability";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        info!("Handling command: {:?}", command);

        let events = match command {
            RefreshCapabilityCommand::CreateRefreshCapability {
                refresh_reference,
                credential_id,
            } => {
                if self.created_at.is_some() {
                    return Err(RefreshCapabilityError::AlreadyExists);
                }

                #[cfg(feature = "test_utils")]
                let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z".parse().map_err(|e| {
                    RefreshCapabilityError::BuildRefreshCapabilityError(format!("Failed to parse created_at: {e}"))
                })?;
                #[cfg(not(feature = "test_utils"))]
                let created_at: DateTime<Utc> = chrono::Utc::now();

                vec![RefreshCapabilityCreated {
                    refresh_reference,
                    credential_id,
                    created_at,
                }]
            }
            RefreshCapabilityCommand::DisableRefreshCapability => {
                if self.created_at.is_none() {
                    return Err(RefreshCapabilityError::NotFound);
                }

                if self.status == RefreshCapabilityStatus::Disabled {
                    return Err(RefreshCapabilityError::AlreadyDisabled);
                }

                vec![RefreshCapabilityDisabled]
            }
        };
        for event in events {
            sink.write(event, self).await;
        }
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        debug!("Applying event: {:?}", event);

        match event {
            RefreshCapabilityCreated {
                refresh_reference,
                credential_id,
                created_at,
            } => {
                self.refresh_reference = refresh_reference;
                self.credential_id = credential_id;
                self.status = RefreshCapabilityStatus::Active;
                self.created_at = Some(created_at);
            }
            RefreshCapabilityDisabled => {
                self.status = RefreshCapabilityStatus::Disabled;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::IssuanceServices;
    use agent_secret_manager::service::Service;

    fn create_refresh_capability_command() -> RefreshCapabilityCommand {
        RefreshCapabilityCommand::CreateRefreshCapability {
            refresh_reference: "refresh-reference".to_string(),
            credential_id: "credential-id".to_string(),
        }
    }

    #[async_std::test]
    async fn create_refresh_capability_records_reference() {
        let sink = EventSink::default();
        let services = IssuanceServices::default().await;
        let mut refresh_capability = RefreshCapability::default();

        refresh_capability
            .handle(create_refresh_capability_command(), &services, &sink)
            .await
            .expect("refresh capability creation should succeed");
        let events = sink.collect().await;
        let sink = EventSink::default();

        assert_eq!(events.len(), 1);

        let RefreshCapabilityCreated {
            refresh_reference,
            credential_id,
            created_at,
        } = events
            .first()
            .expect("refresh capability creation should emit an event")
            .clone()
        else {
            panic!("expected refresh capability created event");
        };

        assert_eq!(refresh_reference, "refresh-reference");
        assert_eq!(credential_id, "credential-id");

        refresh_capability.apply(RefreshCapabilityCreated {
            refresh_reference,
            credential_id,
            created_at,
        });

        assert_eq!(refresh_capability.refresh_reference, "refresh-reference");
        assert_eq!(refresh_capability.credential_id, "credential-id");
        assert_eq!(refresh_capability.status, RefreshCapabilityStatus::Active);
        assert!(refresh_capability.created_at.is_some());

        let error = refresh_capability
            .handle(create_refresh_capability_command(), &services, &sink)
            .await
            .expect_err("existing refresh capability should not be recreated");

        assert_eq!(error, RefreshCapabilityError::AlreadyExists);
    }

    #[async_std::test]
    async fn disable_refresh_capability_marks_reference_disabled() {
        let sink = EventSink::default();
        let services = IssuanceServices::default().await;
        let mut refresh_capability = RefreshCapability::default();

        refresh_capability
            .handle(create_refresh_capability_command(), &services, &sink)
            .await
            .expect("refresh capability creation should succeed");
        let events = sink.collect().await;
        let sink = EventSink::default();

        for event in events {
            refresh_capability.apply(event);
        }

        refresh_capability
            .handle(RefreshCapabilityCommand::DisableRefreshCapability, &services, &sink)
            .await
            .expect("refresh capability disable should succeed");
        let events = sink.collect().await;

        assert_eq!(events, vec![RefreshCapabilityDisabled]);

        for event in events {
            refresh_capability.apply(event);
        }

        assert_eq!(refresh_capability.status, RefreshCapabilityStatus::Disabled);
    }
}
