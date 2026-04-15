use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::HolderServices;
use agent_shared::credential_status_checker::CredentialStatusChecker;
use agent_shared::get_unverified_jwt_claims;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use identity_credential::credential::Jwt;
use oid4vc_core::credential_status_verifier::CredentialStatusVerifier;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, utoipa::ToSchema)]
pub struct Data {
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[schema(as = CredentialView)]
pub struct Credential {
    #[serde(rename = "id")]
    pub holder_credential_id: String,
    pub received_offer_id: Option<String>,
    #[schema(value_type = Option<String>)]
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
        "holder_credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            AddCredential {
                holder_credential_id,
                received_offer_id,
                credential,
            } => {
                let raw = get_unverified_jwt_claims(&serde_json::json!(credential))
                    .ok_or(CredentialError::CredentialDecodingError)?;

                if let Some(status_claim) = raw.get("status") {
                    let credential_status_checker = CredentialStatusChecker {
                        verification_material_resolver: services.holder.clone(),
                    };

                    credential_status_checker
                        .check_credential_status(status_claim.to_owned())
                        .await
                        .map_err(|_| CredentialError::InvalidCredentialStatus)?;
                }

                let raw_credential = raw.get("vc").cloned().ok_or(CredentialDecodingError)?;

                Ok(vec![CredentialAdded {
                    holder_credential_id,
                    received_offer_id,
                    credential,
                    data: Data { raw: raw_credential },
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            CredentialAdded {
                holder_credential_id,
                received_offer_id,
                credential,
                data,
            } => {
                self.holder_credential_id = holder_credential_id;
                self.received_offer_id = received_offer_id;
                self.signed = Some(credential);
                self.data = Some(data);
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
    use crate::offer::aggregate::test_utils::received_offer_id;
    use agent_issuance::credential::aggregate::test_utils::JWT_VC_JSON_OBV3_JWT;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[serial_test::serial]
    async fn test_add_credential(holder_credential_id: String, received_offer_id: String) {
        CredentialTestFramework::with(HolderServices::default().await)
            .given_no_previous_events()
            .when(CredentialCommand::AddCredential {
                holder_credential_id: holder_credential_id.clone(),
                received_offer_id: Some(received_offer_id.clone()),
                credential: Jwt::from(JWT_VC_JSON_OBV3_JWT.to_string()),
            })
            .then_expect_events(vec![CredentialEvent::CredentialAdded {
                holder_credential_id,
                received_offer_id: Some(received_offer_id),
                credential: Jwt::from(JWT_VC_JSON_OBV3_JWT.to_string()),
                data: Data {
                    raw: get_unverified_jwt_claims(&serde_json::json!(JWT_VC_JSON_OBV3_JWT)).unwrap()["vc"].clone(),
                },
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_shared::generate_random_string;
    use rstest::*;

    #[fixture]
    pub fn holder_credential_id() -> String {
        generate_random_string()
    }
}
