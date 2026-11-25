use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::credential::signed::{verify_sd_jwt_credential, SdJwtCredentialError, SignedCredential};
use crate::services::HolderServices;
use agent_shared::credential_status_checker::CredentialStatusChecker;
use cqrs_es::{event_sink::EventSink, Aggregate};
use oid4vc_core::utils::jwt::get_unverified_jwt_claims;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    pub signed: Option<SignedCredential>,
    pub data: Option<Data>,
}

impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<HolderServices>;

    const TYPE: &'static str = "holder_credential";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        let events: Vec<Self::Event> = match command {
            AddCredential {
                holder_credential_id,
                received_offer_id,
                credential,
            } => {
                let (raw_credential, status_claim) = match &credential {
                    SignedCredential::Jwt(jwt) => {
                        let raw = get_unverified_jwt_claims(&serde_json::json!(jwt))
                            .map_err(|_| CredentialError::CredentialDecodingError)?;
                        (
                            raw.get("vc").cloned().ok_or(CredentialDecodingError)?,
                            raw.get("status").cloned(),
                        )
                    }
                    SignedCredential::SdJwt(sd_jwt) => {
                        let verified = verify_sd_jwt_credential(sd_jwt, &*services.holder)
                            .await
                            .map_err(|error| match error {
                                SdJwtCredentialError::UnsupportedFormat(_) => CredentialDecodingError,
                                SdJwtCredentialError::Invalid(reason) => InvalidCredential(reason),
                            })?;
                        let raw = serde_json::to_value(verified.credential()).map_err(|_| CredentialDecodingError)?;
                        let status = verified
                            .status()
                            .map(serde_json::to_value)
                            .transpose()
                            .map_err(|_| CredentialDecodingError)?;
                        (raw, status)
                    }
                };

                if let Some(status_claim) = status_claim {
                    let credential_status_checker = CredentialStatusChecker {
                        key_resolver: services.holder.clone(),
                    };

                    credential_status_checker
                        .check_credential_status(status_claim)
                        .await
                        .map_err(|_| CredentialError::InvalidCredentialStatus)?;
                }

                Ok(vec![CredentialAdded {
                    holder_credential_id,
                    received_offer_id,
                    credential,
                    data: Data { raw: raw_credential },
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

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
    use crate::credential::signed::{issue_vc_sd_jwt, with_forged_signature};
    use crate::offer::aggregate::test_utils::received_offer_id;
    use agent_issuance::credential::aggregate::test_utils::JWT_VC_JSON_OBV3_JWT;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_add_credential(holder_credential_id: String, received_offer_id: String) {
        CredentialTestFramework::with(HolderServices::default().await)
            .given_no_previous_events()
            .when_async(CredentialCommand::AddCredential {
                holder_credential_id: holder_credential_id.clone(),
                received_offer_id: Some(received_offer_id.clone()),
                credential: JWT_VC_JSON_OBV3_JWT.parse().unwrap(),
            })
            .await
            .then_expect_events(vec![CredentialEvent::CredentialAdded {
                holder_credential_id,
                received_offer_id: Some(received_offer_id),
                credential: JWT_VC_JSON_OBV3_JWT.parse().unwrap(),
                data: Data {
                    raw: get_unverified_jwt_claims(&serde_json::json!(JWT_VC_JSON_OBV3_JWT)).unwrap()["vc"].clone(),
                },
            }])
    }

    /// A `vc+sd-jwt` credential is stored once its issuer signature verifies, its data the credential with the
    /// disclosures it carries applied.
    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_add_vc_sd_jwt_credential(holder_credential_id: String, received_offer_id: String) {
        let services = HolderServices::default().await;
        let sd_jwt = issue_vc_sd_jwt(&services.holder, chrono::Utc::now() + chrono::Days::new(1)).await;
        let raw = serde_json::to_value(
            identity_kit::sd_jwt_vc::SdJwtCredentialV2::from_sd_jwt(sd_jwt.clone())
                .unwrap()
                .credential(),
        )
        .unwrap();
        assert_eq!(raw["credentialSubject"]["name"], "Example LLC");

        CredentialTestFramework::with(services)
            .given_no_previous_events()
            .when_async(CredentialCommand::AddCredential {
                holder_credential_id: holder_credential_id.clone(),
                received_offer_id: Some(received_offer_id.clone()),
                credential: SignedCredential::SdJwt(sd_jwt.clone()),
            })
            .await
            .then_expect_events(vec![CredentialEvent::CredentialAdded {
                holder_credential_id,
                received_offer_id: Some(received_offer_id),
                credential: SignedCredential::SdJwt(sd_jwt),
                data: Data { raw },
            }])
    }

    #[rstest]
    #[case::forged_signature(true, 1)]
    #[case::expired(false, -1)]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_add_invalid_vc_sd_jwt_credential(
        holder_credential_id: String,
        #[case] forged: bool,
        #[case] days_valid: i64,
    ) {
        let services = HolderServices::default().await;
        let mut sd_jwt = issue_vc_sd_jwt(
            &services.holder,
            chrono::Utc::now() + chrono::Duration::days(days_valid),
        )
        .await;
        if forged {
            sd_jwt = with_forged_signature(&sd_jwt);
        }

        let error = CredentialTestFramework::with(services)
            .given_no_previous_events()
            .when_async(CredentialCommand::AddCredential {
                holder_credential_id,
                received_offer_id: None,
                credential: SignedCredential::SdJwt(sd_jwt),
            })
            .await
            .inspect_result()
            .unwrap_err();
        assert!(matches!(error, CredentialError::InvalidCredential(_)), "{error:?}");
    }

    /// An SD-JWT VC (`dc+sd-jwt`) is not stored.
    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_add_sd_jwt_credential(holder_credential_id: String, received_offer_id: String) {
        CredentialTestFramework::with(HolderServices::default().await)
            .given_no_previous_events()
            .when_async(CredentialCommand::AddCredential {
                holder_credential_id,
                received_offer_id: Some(received_offer_id),
                credential: crate::credential::signed::SD_JWT.parse().unwrap(),
            })
            .await
            .then_expect_error_message(&CredentialError::CredentialDecodingError.to_string())
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
