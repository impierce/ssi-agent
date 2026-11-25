use crate::{
    refresh::{command::RefreshCapabilityCommand, error::RefreshCapabilityError, event::RefreshCapabilityEvent},
    services::IssuanceServices,
};
use chrono::{DateTime, Utc};
use cqrs_es::{event_sink::EventSink, Aggregate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What a refresh token unlocks: preparing replacements of the credential that carries it.
///
/// Its identifier is the SHA-256 of the token ([`RefreshToken::capability_id`](crate::refresh::token::RefreshToken::capability_id)),
/// so the token itself is never stored. A capability is retired once its holder accepts any of its continuations;
/// retirement is recorded lazily, by the next refresh with the token.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct RefreshCapability {
    /// The credential that embeds the token.
    pub credential_id: String,
    pub credential_configuration_id: String,
    /// The replacements prepared from this capability, oldest first. The newest is the pending one while its
    /// credential has not been delivered.
    pub continuations: Vec<Continuation>,
    pub retired: bool,
    pub created_at: Option<DateTime<Utc>>,
}

/// A replacement prepared from a capability: the reissuance relation, its credential and the offer for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Continuation {
    pub reissuance_id: String,
    pub credential_id: String,
    pub offer_id: String,
}

impl RefreshCapability {
    /// The newest continuation, if any.
    pub fn newest_continuation(&self) -> Option<&Continuation> {
        self.continuations.last()
    }
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
        let event = match command {
            RefreshCapabilityCommand::CreateRefreshCapability {
                credential_id,
                credential_configuration_id,
            } => {
                if self.created_at.is_some() {
                    return Err(RefreshCapabilityError::AlreadyExists);
                }
                #[cfg(feature = "test_utils")]
                let created_at = DateTime::<Utc>::UNIX_EPOCH;
                #[cfg(not(feature = "test_utils"))]
                let created_at = Utc::now();

                RefreshCapabilityEvent::RefreshCapabilityCreated {
                    credential_id,
                    credential_configuration_id,
                    created_at,
                }
            }
            RefreshCapabilityCommand::RecordContinuation { after, continuation } => {
                if self.created_at.is_none() {
                    return Err(RefreshCapabilityError::NotFound);
                }
                if self.retired {
                    return Err(RefreshCapabilityError::Retired);
                }
                let newest = self
                    .newest_continuation()
                    .map(|continuation| &continuation.reissuance_id);
                if newest != after.as_ref() {
                    return Err(RefreshCapabilityError::ContinuationSuperseded);
                }
                RefreshCapabilityEvent::ContinuationRecorded { continuation }
            }
            RefreshCapabilityCommand::RetireRefreshCapability { accepted_credential_id } => {
                if self.created_at.is_none() {
                    return Err(RefreshCapabilityError::NotFound);
                }
                if self.retired {
                    return Ok(());
                }
                RefreshCapabilityEvent::RefreshCapabilityRetired { accepted_credential_id }
            }
        };
        sink.write(event, self).await;
        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            RefreshCapabilityEvent::RefreshCapabilityCreated {
                credential_id,
                credential_configuration_id,
                created_at,
            } => {
                self.credential_id = credential_id;
                self.credential_configuration_id = credential_configuration_id;
                self.created_at = Some(created_at);
            }
            RefreshCapabilityEvent::ContinuationRecorded { continuation } => self.continuations.push(continuation),
            RefreshCapabilityEvent::RefreshCapabilityRetired { .. } => self.retired = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refresh::token::RefreshToken;
    use agent_secret_manager::service::Service;

    fn continuation(reissuance_id: &str) -> Continuation {
        Continuation {
            reissuance_id: reissuance_id.to_owned(),
            credential_id: format!("{reissuance_id}-credential"),
            offer_id: format!("{reissuance_id}-offer"),
        }
    }

    fn created() -> RefreshCapability {
        let mut capability = RefreshCapability::default();
        capability.apply(RefreshCapabilityEvent::RefreshCapabilityCreated {
            credential_id: "original".to_owned(),
            credential_configuration_id: "configuration".to_owned(),
            created_at: DateTime::<Utc>::UNIX_EPOCH,
        });
        capability
    }

    async fn handle(
        capability: &mut RefreshCapability,
        command: RefreshCapabilityCommand,
    ) -> Result<Vec<RefreshCapabilityEvent>, RefreshCapabilityError> {
        let sink = EventSink::default();
        capability
            .handle(command, &IssuanceServices::default().await, &sink)
            .await?;
        Ok(sink.collect().await)
    }

    #[tokio::test]
    async fn capability_events_never_contain_the_token() {
        let token = RefreshToken::from("refresh-token".to_owned());
        let mut capability = RefreshCapability::default();
        let mut events = handle(
            &mut capability,
            RefreshCapabilityCommand::CreateRefreshCapability {
                credential_id: "original".to_owned(),
                credential_configuration_id: "configuration".to_owned(),
            },
        )
        .await
        .unwrap();
        for event in events.clone() {
            capability.apply(event);
        }
        events.extend(
            handle(
                &mut capability,
                RefreshCapabilityCommand::RecordContinuation {
                    after: None,
                    continuation: continuation("first"),
                },
            )
            .await
            .unwrap(),
        );

        let stored = serde_json::to_string(&events).unwrap();

        assert!(!stored.contains("refresh-token") && !stored.contains(&token.capability_id()));
    }

    #[tokio::test]
    async fn records_the_pending_offer_of_a_continuation() {
        let mut capability = created();
        capability.apply(RefreshCapabilityEvent::ContinuationRecorded {
            continuation: continuation("first"),
        });

        let events = handle(
            &mut capability,
            RefreshCapabilityCommand::RecordContinuation {
                after: Some("first".to_owned()),
                continuation: continuation("second"),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            events,
            vec![RefreshCapabilityEvent::ContinuationRecorded {
                continuation: continuation("second")
            }]
        );
    }

    #[tokio::test]
    async fn a_continuation_after_an_outdated_one_is_superseded() {
        let mut capability = created();
        capability.apply(RefreshCapabilityEvent::ContinuationRecorded {
            continuation: continuation("first"),
        });

        let result = handle(
            &mut capability,
            RefreshCapabilityCommand::RecordContinuation {
                after: None,
                continuation: continuation("second"),
            },
        )
        .await;

        assert_eq!(result, Err(RefreshCapabilityError::ContinuationSuperseded));
    }

    #[tokio::test]
    async fn a_retired_capability_refuses_to_continue() {
        let mut capability = created();
        capability.apply(RefreshCapabilityEvent::RefreshCapabilityRetired {
            accepted_credential_id: "first-credential".to_owned(),
        });

        let result = handle(
            &mut capability,
            RefreshCapabilityCommand::RecordContinuation {
                after: None,
                continuation: continuation("second"),
            },
        )
        .await;

        assert_eq!(result, Err(RefreshCapabilityError::Retired));
    }
}
