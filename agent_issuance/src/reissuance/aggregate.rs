use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::{
    reissuance::{
        command::ReissuanceCommand,
        error::ReissuanceError,
        event::ReissuanceEvent::{self, ReissuanceCreated},
    },
    services::IssuanceServices,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, utoipa::ToSchema)]
pub struct Reissuance {
    #[serde(rename = "id")]
    pub reissuance_id: String,
    pub original_credential_id: String,
    pub new_credential_id: String,
    pub offer_id: String,
    pub credential_configuration_id: String,

    pub reason: Option<String>,
    pub trigger_type: Option<String>,
    pub triggered_by: Option<String>,
    pub status_action: Option<String>,

    pub created_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl Aggregate for Reissuance {
    type Command = ReissuanceCommand;
    type Event = ReissuanceEvent;
    type Error = ReissuanceError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "reissuance".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        info!("Handling command: {:?}", command);

        match command {
            ReissuanceCommand::CreateReissuance {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
                credential_configuration_id,
                reason,
                trigger_type,
                triggered_by,
                status_action,
            } => {
                if self.created_at.is_some() {
                    return Err(ReissuanceError::AlreadyExists);
                }

                #[cfg(feature = "test_utils")]
                let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z"
                    .parse()
                    .map_err(|e| ReissuanceError::BuildReissuanceError(format!("Failed to parse created_at: {}", e)))?;
                #[cfg(not(feature = "test_utils"))]
                let created_at: DateTime<Utc> = chrono::Utc::now();

                Ok(vec![ReissuanceCreated {
                    created_at,
                    credential_configuration_id,
                    new_credential_id,
                    offer_id,
                    original_credential_id,
                    reason,
                    reissuance_id,
                    status_action,
                    trigger_type,
                    triggered_by,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        debug!("Applying event: {:?}", event);

        match event {
            ReissuanceCreated {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
                credential_configuration_id,
                reason,
                trigger_type,
                triggered_by,
                status_action,
                created_at,
            } => {
                self.reissuance_id = reissuance_id;
                self.original_credential_id = original_credential_id;
                self.new_credential_id = new_credential_id;
                self.offer_id = offer_id;
                self.credential_configuration_id = credential_configuration_id;
                self.reason = reason;
                self.trigger_type = trigger_type;
                self.triggered_by = triggered_by;
                self.status_action = status_action;
                self.created_at = Some(created_at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::IssuanceServices;
    use agent_secret_manager::service::Service;

    fn create_reissuance_command() -> ReissuanceCommand {
        ReissuanceCommand::CreateReissuance {
            reissuance_id: "reissuance-id".to_string(),
            original_credential_id: "original-credential-id".to_string(),
            new_credential_id: "new-credential-id".to_string(),
            offer_id: "offer-id".to_string(),
            credential_configuration_id: "credential-configuration-id".to_string(),
            reason: Some("data_changed".to_string()),
            trigger_type: Some("manual".to_string()),
            triggered_by: Some("unitrust".to_string()),
            status_action: None,
        }
    }

    #[async_std::test]
    async fn create_reissuance_records_relation() {
        let services = IssuanceServices::default().await;
        let mut reissuance = Reissuance::default();

        let events = reissuance
            .handle(create_reissuance_command(), &services)
            .await
            .expect("reissuance creation should succeed");

        assert_eq!(events.len(), 1);

        let ReissuanceCreated {
            reissuance_id,
            original_credential_id,
            new_credential_id,
            offer_id,
            credential_configuration_id,
            reason,
            trigger_type,
            triggered_by,
            status_action,
            created_at,
        } = events
            .first()
            .expect("reissuance creation should emit an event")
            .clone();

        assert_eq!(reissuance_id, "reissuance-id");
        assert_eq!(original_credential_id, "original-credential-id");
        assert_eq!(new_credential_id, "new-credential-id");
        assert_eq!(offer_id, "offer-id");
        assert_eq!(credential_configuration_id, "credential-configuration-id");
        assert_eq!(reason.as_deref(), Some("data_changed"));
        assert_eq!(trigger_type.as_deref(), Some("manual"));
        assert_eq!(triggered_by.as_deref(), Some("unitrust"));
        assert_eq!(status_action, None);

        reissuance.apply(ReissuanceCreated {
            reissuance_id,
            original_credential_id,
            new_credential_id,
            offer_id,
            credential_configuration_id,
            reason,
            trigger_type,
            triggered_by,
            status_action,
            created_at,
        });

        assert_eq!(reissuance.reissuance_id, "reissuance-id");
        assert_eq!(reissuance.original_credential_id, "original-credential-id");
        assert_eq!(reissuance.new_credential_id, "new-credential-id");
        assert_eq!(reissuance.offer_id, "offer-id");
        assert_eq!(reissuance.credential_configuration_id, "credential-configuration-id");
        assert_eq!(reissuance.reason.as_deref(), Some("data_changed"));
        assert_eq!(reissuance.trigger_type.as_deref(), Some("manual"));
        assert_eq!(reissuance.triggered_by.as_deref(), Some("unitrust"));
        assert_eq!(reissuance.status_action, None);
        assert!(reissuance.created_at.is_some());

        let error = reissuance
            .handle(create_reissuance_command(), &services)
            .await
            .expect_err("existing reissuance relation should not be recreated");

        assert_eq!(error, ReissuanceError::AlreadyExists);
    }

    #[async_std::test]
    async fn create_reissuance_preserves_optional_status_action_metadata() {
        let services = IssuanceServices::default().await;
        let mut command = create_reissuance_command();
        let ReissuanceCommand::CreateReissuance { status_action, .. } = &mut command;
        *status_action = Some("replaced".to_string());

        let events = Reissuance::default()
            .handle(command, &services)
            .await
            .expect("reissuance creation should succeed");

        let ReissuanceCreated { status_action, .. } = events
            .first()
            .expect("reissuance creation should emit an event")
            .clone();

        assert_eq!(status_action.as_deref(), Some("replaced"));
    }
}
