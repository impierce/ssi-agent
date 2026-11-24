use std::sync::Arc;

use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};

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

    pub created_at: Option<DateTime<Utc>>,
}

impl Aggregate for Reissuance {
    type Command = ReissuanceCommand;
    type Event = ReissuanceEvent;
    type Error = ReissuanceError;
    type Services = Arc<IssuanceServices>;

    const TYPE: &'static str = "reissuance";

    async fn handle(
        &mut self,
        command: Self::Command,
        _services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        let events = match command {
            ReissuanceCommand::CreateReissuance {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
                credential_configuration_id,
                reason,
                trigger_type,
                triggered_by,
            } => {
                if self.created_at.is_some() {
                    return Err(ReissuanceError::AlreadyExists);
                }

                #[cfg(feature = "test_utils")]
                let created_at: DateTime<Utc> = "2010-01-01T00:00:00Z"
                    .parse()
                    .map_err(|error| ReissuanceError::Failed(format!("Failed to parse created_at: {error}")))?;
                #[cfg(not(feature = "test_utils"))]
                let created_at: DateTime<Utc> = chrono::Utc::now();

                vec![ReissuanceCreated {
                    created_at,
                    credential_configuration_id,
                    new_credential_id,
                    offer_id,
                    original_credential_id,
                    reason,
                    reissuance_id,
                    trigger_type,
                    triggered_by,
                }]
            }
        };
        for event in events {
            sink.write(event, self).await;
        }
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
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
                self.created_at = Some(created_at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_secret_manager::service::Service;

    #[tokio::test]
    async fn records_the_relation_from_original_to_replacement() {
        let sink = EventSink::default();
        let mut reissuance = Reissuance::default();

        reissuance
            .handle(
                ReissuanceCommand::CreateReissuance {
                    reissuance_id: "reissuance-id".to_string(),
                    original_credential_id: "original-credential-id".to_string(),
                    new_credential_id: "new-credential-id".to_string(),
                    offer_id: "offer-id".to_string(),
                    credential_configuration_id: "credential-configuration-id".to_string(),
                    reason: Some("data_changed".to_string()),
                    trigger_type: Some("manual".to_string()),
                    triggered_by: Some("unitrust".to_string()),
                },
                &IssuanceServices::default().await,
                &sink,
            )
            .await
            .unwrap();

        assert_eq!(
            sink.collect().await,
            vec![ReissuanceCreated {
                reissuance_id: "reissuance-id".to_string(),
                original_credential_id: "original-credential-id".to_string(),
                new_credential_id: "new-credential-id".to_string(),
                offer_id: "offer-id".to_string(),
                credential_configuration_id: "credential-configuration-id".to_string(),
                reason: Some("data_changed".to_string()),
                trigger_type: Some("manual".to_string()),
                triggered_by: Some("unitrust".to_string()),
                created_at: "2010-01-01T00:00:00Z".parse().unwrap(),
            }]
        );
    }
}
