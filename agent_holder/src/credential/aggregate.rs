use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::HolderServices;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Credential {
    pub credential_id: Option<String>,
    pub offer_id: Option<String>,
    pub credential: Option<serde_json::Value>,
}

#[async_trait]
impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<HolderServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use CredentialCommand::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddCredential {
                credential_id,
                offer_id,
                credential,
            } => Ok(vec![CredentialAdded {
                credential_id,
                offer_id,
                credential,
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            CredentialAdded {
                credential_id,
                offer_id,
                credential,
            } => {
                self.credential_id = Some(credential_id);
                self.offer_id = Some(offer_id);
                self.credential = Some(credential);
            }
        }
    }
}

#[cfg(test)]
pub mod credential_tests {
    use super::test_utils::*;
    use super::*;
    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;
    use crate::offer::aggregate::test_utils::offer_id;
    use agent_issuance::credential::aggregate::test_utils::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;
    use serde_json::json;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[serial_test::serial]
    fn test_add_credential(credential_id: String, offer_id: String) {
        CredentialTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(CredentialCommand::AddCredential {
                credential_id: credential_id.clone(),
                offer_id: offer_id.clone(),
                credential: json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT),
            })
            .then_expect_events(vec![CredentialEvent::CredentialAdded {
                credential_id,
                offer_id,
                credential: json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT),
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_shared::generate_random_string;
    use rstest::*;

    #[fixture]
    pub fn credential_id() -> String {
        generate_random_string()
    }
}
