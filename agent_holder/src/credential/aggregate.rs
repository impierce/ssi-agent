use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::HolderServices;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cqrs_es::Aggregate;
use identity_credential::credential::Jwt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Data {
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Credential {
    pub credential_id: Option<String>,
    pub offer_id: Option<String>,
    pub signed: Option<Jwt>,
    pub data: Option<Data>,
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
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddCredential {
                credential_id,
                offer_id,
                credential,
            } => {
                let raw = get_unverified_jwt_claims(&serde_json::json!(credential))?
                    .get("vc")
                    .cloned()
                    .ok_or(CredentialDecodingError)?;

                Ok(vec![CredentialAdded {
                    credential_id,
                    offer_id,
                    credential,
                    data: Data { raw },
                }])
            }
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
                data,
            } => {
                self.credential_id = Some(credential_id);
                self.offer_id = Some(offer_id);
                self.signed = Some(credential);
                self.data = Some(data);
            }
        }
    }
}

// TODO: actually validate the JWT!
/// Get the claims from a JWT without performing validation.
pub fn get_unverified_jwt_claims(jwt: &serde_json::Value) -> Result<serde_json::Value, CredentialError> {
    jwt.as_str()
        .and_then(|string| string.splitn(3, '.').collect::<Vec<&str>>().get(1).cloned())
        .and_then(|payload| {
            URL_SAFE_NO_PAD
                .decode(payload)
                .ok()
                .and_then(|payload_bytes| serde_json::from_slice::<serde_json::Value>(&payload_bytes).ok())
        })
        .ok_or(CredentialError::CredentialDecodingError)
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

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[serial_test::serial]
    fn test_add_credential(credential_id: String, offer_id: String) {
        CredentialTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(CredentialCommand::AddCredential {
                credential_id: credential_id.clone(),
                offer_id: offer_id.clone(),
                credential: Jwt::from(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string()),
            })
            .then_expect_events(vec![CredentialEvent::CredentialAdded {
                credential_id,
                offer_id,
                credential: Jwt::from(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string()),
                data: Data {
                    raw: get_unverified_jwt_claims(&serde_json::json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT)).unwrap()
                        ["vc"]
                        .clone(),
                },
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
