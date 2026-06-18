use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
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

#[async_trait]
impl Aggregate for RefreshCapability {
    type Command = RefreshCapabilityCommand;
    type Event = RefreshCapabilityEvent;
    type Error = RefreshCapabilityError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "refresh_capability".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        info!("Handling command: {:?}", command);

        match command {
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

                Ok(vec![RefreshCapabilityCreated {
                    refresh_reference,
                    credential_id,
                    created_at,
                }])
            }
            RefreshCapabilityCommand::DisableRefreshCapability => {
                if self.created_at.is_none() {
                    return Err(RefreshCapabilityError::NotFound);
                }

                if self.status == RefreshCapabilityStatus::Disabled {
                    return Err(RefreshCapabilityError::AlreadyDisabled);
                }

                Ok(vec![RefreshCapabilityDisabled])
            }
        }
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
        let services = IssuanceServices::default().await;
        let mut refresh_capability = RefreshCapability::default();

        let events = refresh_capability
            .handle(create_refresh_capability_command(), &services)
            .await
            .expect("refresh capability creation should succeed");

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
            .handle(create_refresh_capability_command(), &services)
            .await
            .expect_err("existing refresh capability should not be recreated");

        assert_eq!(error, RefreshCapabilityError::AlreadyExists);
    }

    #[async_std::test]
    async fn disable_refresh_capability_marks_reference_disabled() {
        let services = IssuanceServices::default().await;
        let mut refresh_capability = RefreshCapability::default();

        let events = refresh_capability
            .handle(create_refresh_capability_command(), &services)
            .await
            .expect("refresh capability creation should succeed");

        for event in events {
            refresh_capability.apply(event);
        }

        let events = refresh_capability
            .handle(RefreshCapabilityCommand::DisableRefreshCapability, &services)
            .await
            .expect("refresh capability disable should succeed");

        assert_eq!(events, vec![RefreshCapabilityDisabled]);

        for event in events {
            refresh_capability.apply(event);
        }

        assert_eq!(refresh_capability.status, RefreshCapabilityStatus::Disabled);
    }
}
