use super::entity::Data;
use crate::credential::command::CredentialCommand;
use crate::credential::error::CredentialError::{self};
use crate::credential::event::CredentialEvent;
use crate::services::IssuanceServices;
use agent_shared::config::{config, get_preferred_did_method, get_preferred_signing_algorithm};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use identity_core::convert::FromJson;
use identity_credential::credential::{
    Credential as W3CVerifiableCredential, CredentialBuilder as W3CVerifiableCredentialBuilder, Issuer,
};
use jsonwebtoken::Header;
use oid4vc_core::jwt;
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::{
    CredentialDefinition, JwtVcJson, JwtVcJsonParameters,
};
use oid4vci::credential_format_profiles::{CredentialFormats, Parameters};
use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
use oid4vci::notification_request::NotificationRequest;
use oid4vci::VerifiableCredentialJwt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info};
use types_ob_v3::prelude::{
    AchievementCredential, AchievementCredentialBuilder, AchievementCredentialType, AchievementSubject, Profile,
    ProfileBuilder,
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Pending,
    Issued,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum CredentialExpiry {
    Fixed(chrono::DateTime<chrono::Utc>),
    #[serde(with = "never_as_str")]
    Never,
}

mod never_as_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("never")
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "never" {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected 'never'"))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    #[serde(rename = "id")]
    pub credential_id: String,
    pub notification_id: Option<String>,
    pub data: Option<Data>,
    pub credential_configuration: CredentialConfigurationsSupportedObject,
    pub signed: Option<serde_json::Value>,
    pub status: Status,
    pub holder_notifications: Vec<NotificationRequest>,
}

#[async_trait]
impl Aggregate for Credential {
    type Command = CredentialCommand;
    type Event = CredentialEvent;
    type Error = CredentialError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "credential".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use CredentialCommand::*;
        use CredentialError::*;
        use CredentialEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateUnsignedCredential {
                credential_id,
                data,
                credential_configuration,
                // conv_args: _
                expires_at,
            } => match &credential_configuration.credential_format {
                CredentialFormats::JwtVcJson(Parameters::<JwtVcJson> {
                    parameters:
                        JwtVcJsonParameters {
                            credential_definition: CredentialDefinition { type_, .. },
                            ..
                        },
                }) => {
                    #[cfg(feature = "test_utils")]
                    let notification_id = test_utils::notification_id();
                    #[cfg(not(feature = "test_utils"))]
                    let notification_id = agent_shared::generate_random_string();

                    #[cfg(feature = "test_utils")]
                    let issuance_date = "2010-01-01T00:00:00Z".to_string();
                    #[cfg(not(feature = "test_utils"))]
                    let issuance_date = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

                    let name = config()
                        .display
                        .first()
                        .expect("Configuration `display.name` missing")
                        .name
                        .clone();

                    let issuer: Profile = ProfileBuilder::default()
                        .id(config().url.clone())
                        .type_("Profile")
                        .name(name)
                        .try_into()
                        .expect("Could not build issuer profile");

                    let issuance_date =
                        identity_core::common::Timestamp::parse(&issuance_date).expect("Could not parse issuance_date");

                    let expiration_date = match expires_at {
                        CredentialExpiry::Fixed(fixed) => {
                            let fixed = identity_core::common::Timestamp::from_unix(fixed.timestamp())
                                .map_err(|_| InvalidExpirationDateError)?;

                            Some(fixed)
                        }
                        CredentialExpiry::Never => None,
                    };

                    let mut credential_types: Vec<String> = type_.clone();

                    let id = data
                        .raw
                        .get("id")
                        .map(|id| {
                            id.as_str()
                                .and_then(|id_str| Url::parse(id_str).ok())
                                .ok_or(InvalidIdentifierError)
                        })
                        .transpose()?;

                    let credential_subject = identity_credential::credential::Subject::from_json_value(
                        data.raw["credentialSubject"].clone(),
                    )
                    .map_err(|e| InvalidCredentialSubjectError(e.to_string()))?;

                    // Loop through all the items in the `type` array in reverse until we find a match.
                    while let Some(credential_type) = credential_types.pop() {
                        match credential_type.as_str() {
                            "VerifiableCredential" => {
                                let issuer = match serde_json::from_value::<Issuer>(json!({
                                    "id": issuer.id,
                                    "name": issuer.name,
                                })) {
                                    Ok(issuer) => issuer,
                                    Err(_) => unreachable!("Couldn't parse issuer"),
                                };

                                let builder = W3CVerifiableCredentialBuilder::default()
                                    .issuer(issuer)
                                    .subject(credential_subject)
                                    .issuance_date(issuance_date);

                                let builder = if let Some(expiration_date) = expiration_date {
                                    builder.expiration_date(expiration_date)
                                } else {
                                    builder
                                };

                                let builder = if let Some(id) = id {
                                    builder.id(id.into())
                                } else {
                                    builder
                                };

                                let credential: W3CVerifiableCredential = builder
                                    .build()
                                    .map_err(|e| InvalidCredentialSubjectError(e.to_string()))?;

                                // Set the type to the original credential configuration type.
                                let mut raw = json!(credential);
                                raw["type"] = json!(type_);

                                return Ok(vec![UnsignedCredentialCreated {
                                    credential_id,
                                    data: Data { raw },
                                    credential_configuration,
                                    notification_id: Some(notification_id),
                                }]);
                            }
                            "AchievementCredential" | "OpenBadgeCredential" => {
                                let name = credential_configuration
                                    .display
                                    .first()
                                    .and_then(|display| display.get("name"))
                                    .and_then(|name| name.as_str())
                                    .map(ToString::to_string)
                                    .unwrap_or("OpenBadge Credential".to_string());

                                let credential_subject = serde_json_path_to_error::from_value::<AchievementSubject>(
                                    json!(credential_subject),
                                )
                                .map_err(|e| InvalidCredentialSubjectError(e.to_string()))?;

                                let builder = AchievementCredentialBuilder::default()
                                    .context(vec![
                                        "https://www.w3.org/2018/credentials/v1",
                                        "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json",
                                    ])
                                    .type_(AchievementCredentialType::from(vec![
                                        "VerifiableCredential",
                                        &credential_type,
                                    ]))
                                    .name(name)
                                    .issuer(issuer)
                                    .credential_subject(credential_subject)
                                    .issuance_date(issuance_date.to_rfc3339());

                                let builder = if let Some(expiration_date) = expiration_date {
                                    builder.expiration_date(expiration_date.to_rfc3339())
                                } else {
                                    builder
                                };

                                let builder = builder.id(id.ok_or(InvalidIdentifierError)?);

                                let credential: AchievementCredential =
                                    builder.try_into().map_err(InvalidCredentialSubjectError)?;

                                return Ok(vec![UnsignedCredentialCreated {
                                    credential_id,
                                    notification_id: Some(notification_id),
                                    data: Data { raw: json!(credential) },
                                    credential_configuration,
                                }]);
                            }
                            _ => continue,
                        }
                    }

                    Err(UnsupportedCredentialType)
                }
                _ => Err(UnsupportedCredentialFormat(serde_json::json!(
                    credential_configuration.credential_format
                ))),
            },

            CreateSignedCredential {
                credential_id,
                signed_credential,
            } => {
                #[cfg(feature = "test_utils")]
                let notification_id = test_utils::notification_id();
                #[cfg(not(feature = "test_utils"))]
                let notification_id = agent_shared::generate_random_string();

                Ok(vec![SignedCredentialCreated {
                    credential_id,
                    signed_credential,
                    notification_id: Some(notification_id),
                }])
            }
            SignCredential {
                credential_id,
                subject_id,
                overwrite,
            } => {
                if self.signed.is_some() && !overwrite {
                    return Ok(vec![]);
                }

                let id: Option<Url> = self
                    .data
                    .as_ref()
                    .and_then(|data| data.raw.get("id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| Url::parse(id).ok());

                let default_did_method = get_preferred_did_method();

                let issuer_did = services
                    .issuer
                    .identifier(&default_did_method.to_string(), get_preferred_signing_algorithm())
                    .await
                    .unwrap();
                let signed_credential = {
                    let mut credential = self.data.as_ref().ok_or(MissingCredentialDataError)?.clone();

                    if let Some(ref id) = id {
                        credential.raw["id"] = json!(id);
                    };

                    credential.raw["issuer"] = json!(issuer_did);

                    let credential_subject = credential.raw["credentialSubject"].as_object().unwrap().clone();

                    // Create a new Map and insert the id field first
                    let mut new_credential_subject = serde_json::Map::new();
                    new_credential_subject.insert("id".to_string(), json!(subject_id));

                    // Insert the rest of the fields
                    for (key, value) in credential_subject {
                        if key != "id" {
                            new_credential_subject.insert(key, value);
                        }
                    }

                    info!("Credential subject: {:?}", new_credential_subject);

                    // Replace the original credentialSubject with the new map
                    credential.raw["credentialSubject"] = serde_json::Value::Object(new_credential_subject);

                    info!("Credential: {:?}", credential);

                    #[cfg(feature = "test_utils")]
                    let iat = 1262304000; // 2010-01-01T00:00:00Z
                    #[cfg(not(feature = "test_utils"))]
                    let iat = credential.raw["issuanceDate"]
                        .as_str()
                        .unwrap()
                        .parse::<chrono::DateTime<chrono::Utc>>()
                        .unwrap()
                        .timestamp();

                    let exp = credential.raw["expirationDate"].as_str().map(|expiration_date| {
                        expiration_date
                            .parse::<chrono::DateTime<chrono::Utc>>()
                            .expect("Could not parse `expirationDate` to DateTime")
                            .timestamp()
                    });

                    // Add standard claims
                    let vc_jwt_builder = VerifiableCredentialJwt::builder()
                        .sub(subject_id)
                        .iss(issuer_did)
                        .iat(iat)
                        .nbf(iat); // TODO: setting the `nbf` to `iat` makes the JWT immediately usable

                    let vc_jwt_builder = if let Some(exp) = exp {
                        vc_jwt_builder.exp(exp)
                    } else {
                        vc_jwt_builder
                    };

                    let vc_jwt_builder = if let Some(id) = id {
                        vc_jwt_builder.jti(id.to_string())
                    } else {
                        vc_jwt_builder
                    };

                    json!(jwt::encode(
                        services.issuer.clone(),
                        Header::new(get_preferred_signing_algorithm()),
                        vc_jwt_builder.verifiable_credential(credential.raw).build().ok(),
                        &default_did_method.to_string()
                    )
                    .await
                    .ok())
                };

                Ok(vec![CredentialSigned {
                    credential_id,
                    signed_credential,
                    status: Status::Issued,
                }])
            }
            AddNotification {
                credential_id,
                notification,
            } => Ok(vec![CredentialEvent::NotificationReceived {
                credential_id,
                notification,
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use CredentialEvent::*;

        debug!("Applying event: {:?}", event);

        match event {
            UnsignedCredentialCreated {
                credential_id,
                data,
                credential_configuration,
                notification_id,
            } => {
                self.credential_id = credential_id;
                self.data.replace(data);
                self.credential_configuration = *credential_configuration;
                self.notification_id = notification_id;
            }
            SignedCredentialCreated {
                credential_id,
                signed_credential,
                notification_id,
            } => {
                self.credential_id = credential_id;
                self.signed.replace(signed_credential);
                self.notification_id = notification_id;
            }
            CredentialSigned {
                credential_id,
                signed_credential,
                status,
            } => {
                self.credential_id = credential_id;
                self.signed.replace(signed_credential);
                self.status = status;
            }
            NotificationReceived {
                credential_id,
                notification,
            } => {
                self.credential_id = credential_id;
                self.holder_notifications.push(notification);
            }
        }
    }
}

#[cfg(test)]
pub mod credential_tests {
    use super::test_utils::*;
    use super::*;

    use jsonwebtoken::Algorithm;

    use rstest::rstest;
    use serde_json::json;

    use cqrs_es::test::TestFramework;

    use crate::credential::aggregate::Credential;
    use crate::credential::event::CredentialEvent;
    use crate::offer::aggregate::test_utils::holder;
    use agent_secret_manager::service::Service;
    use oid4vc_core::Subject;

    type CredentialTestFramework = TestFramework<Credential>;

    #[rstest]
    #[case::openbadges(
        OPENBADGE_CREDENTIAL_SUBJECT.clone(),
        OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_OPENBADGE_CREDENTIAL.clone()
    )]
    #[case::w3c_vc(
        W3C_VC_CREDENTIAL_SUBJECT.clone(),
        W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
        UNSIGNED_W3C_VC_CREDENTIAL.clone()
    )]
    #[serial_test::serial]
    async fn test_create_unsigned_credential(
        #[case] credential_subject: serde_json::Value,
        #[case] credential_configuration: CredentialConfigurationsSupportedObject,
        #[case] unsigned_credential: serde_json::Value,
        credential_id: String,
        notification_id: String,
    ) {
        CredentialTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(CredentialCommand::CreateUnsignedCredential {
                credential_id: credential_id.clone(),
                data: Data {
                    raw: credential_subject,
                },
                credential_configuration: Box::new(credential_configuration.clone()),
                expires_at: CredentialExpiry::Never,
            })
            .then_expect_events(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id,
                data: Data {
                    raw: unsigned_credential,
                },
                notification_id: Some(notification_id.clone()),
                credential_configuration: Box::new(credential_configuration),
            }])
    }

    #[rstest]
    #[case::openbadges(
        UNSIGNED_OPENBADGE_CREDENTIAL.clone(),
        OPENBADGE_CREDENTIAL_CONFIGURATION.clone(),
        OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string(),
    )]
    #[case::w3c_vc(
        UNSIGNED_W3C_VC_CREDENTIAL.clone(),
        W3C_VC_CREDENTIAL_CONFIGURATION.clone(),
        W3C_VC_VERIFIABLE_CREDENTIAL_JWT.to_string(),
    )]
    #[serial_test::serial]
    async fn test_sign_credential(
        #[future(awt)] holder: Arc<dyn Subject>,
        #[case] unsigned_credential: serde_json::Value,
        #[case] credential_configuration: CredentialConfigurationsSupportedObject,
        #[case] verifiable_credential_jwt: String,
        credential_id: String,
    ) {
        CredentialTestFramework::with(Service::default())
            .given(vec![CredentialEvent::UnsignedCredentialCreated {
                credential_id: credential_id.clone(),
                data: Data {
                    raw: unsigned_credential,
                },
                credential_configuration: Box::new(credential_configuration),
                notification_id: None,
            }])
            .when(CredentialCommand::SignCredential {
                credential_id: credential_id.clone(),
                subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                overwrite: false,
            })
            .then_expect_events(vec![CredentialEvent::CredentialSigned {
                credential_id,
                signed_credential: json!(verifiable_credential_jwt),
                status: Status::Issued,
            }])
    }

    pub mod expiry_tests {
        use super::*;

        #[test]
        fn custom_serializer_for_credential_expiry() {
            let deserialized: CredentialExpiry = serde_json::from_value(serde_json::json!("never")).unwrap();
            assert_eq!(deserialized, CredentialExpiry::Never);

            let serialized = serde_json::to_value(&CredentialExpiry::Never).unwrap();
            assert_eq!(serialized, serde_json::json!("never"));
        }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use jsonwebtoken::Algorithm;
    use lazy_static::lazy_static;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        proof::KeyProofMetadata,
        ProofType,
    };
    use rstest::fixture;
    use serde_json::json;
    use std::collections::HashMap;

    #[fixture]
    pub fn notification_id() -> String {
        "notification_id".to_string()
    }

    pub const OPENBADGE_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsImp0aSI6Imh0dHBzOi8vZXhhbXBsZS5jb20vY3JlZGVudGlhbHMvMzUyNyIsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHBzOi8vZXhhbXBsZS5jb20vY3JlZGVudGlhbHMvMzUyNyIsInR5cGUiOlsiVmVyaWZpYWJsZUNyZWRlbnRpYWwiLCJPcGVuQmFkZ2VDcmVkZW50aWFsIl0sImlzc3VlciI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwiaXNzdWFuY2VEYXRlIjoiMjAxMC0wMS0wMVQwMDowMDowMFoiLCJuYW1lIjoiVGVhbXdvcmsgQmFkZ2UiLCJjcmVkZW50aWFsU3ViamVjdCI6eyJpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwidHlwZSI6WyJBY2hpZXZlbWVudFN1YmplY3QiXSwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEiOnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY29nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNvZ25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbnQuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.GBbACSRxM_nBdhWqntrVaMA78ftPR0fO0sHM1v5sYMIJUWCrOamo9EN_67nAHuvwl_og6EVz36o1we7U9M6oCA";

    pub const W3C_VC_VERIFIABLE_CREDENTIAL_JWT: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsInN1YiI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0IiwibmJmIjoxMjYyMzA0MDAwLCJpYXQiOjEyNjIzMDQwMDAsInZjIjp7IkBjb250ZXh0IjoiaHR0cHM6Ly93d3cudzMub3JnLzIwMTgvY3JlZGVudGlhbHMvdjEiLCJ0eXBlIjpbIlZlcmlmaWFibGVDcmVkZW50aWFsIl0sImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJmaXJzdF9uYW1lIjoiRmVycmlzIiwibGFzdF9uYW1lIjoiUnVzdGFjZWFuIiwiZGVncmVlIjp7InR5cGUiOiJNYXN0ZXJEZWdyZWUiLCJuYW1lIjoiTWFzdGVyIG9mIE9jZWFub2dyYXBoeSJ9fSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rZ0U4NE5DTXBNZUF4OWpLOWNmNVc0RzhnY1o5eHV3SnZHMWU3d05rOEtDZ3QiLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiJ9fQ.Zhdom31SC8oh7h4DHz6hoTQeHIV5iA2jUO8gbt3KfYeZFiPbmJiBWNBy3ZmcSJ961YNOdK0I5MWqDw5nlsZpDw";

    #[fixture]
    pub fn credential_id() -> String {
        "credential_id".to_string()
    }

    lazy_static! {
        pub static ref OPENBADGE_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (
                        CredentialDefinition {
                            type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                            credential_subject: Default::default(),
                        },
                        None,
                    )
                        .into(),
                }),
                cryptographic_binding_methods_supported: vec!["did:key".to_string(), "did:jwk".to_string(),],
                credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
                    },
                )]),
                display: vec![json!({
                    "name": "Teamwork Badge",
                    "logo": {
                        "url": "https://example.com/logo.png"
                    }
                })],
                ..Default::default()
            };
        pub static ref W3C_VC_CREDENTIAL_CONFIGURATION: CredentialConfigurationsSupportedObject =
            CredentialConfigurationsSupportedObject {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    parameters: (
                        CredentialDefinition {
                            type_: vec!["VerifiableCredential".to_string()],
                            credential_subject: Default::default(),
                        },
                        None,
                    )
                        .into(),
                }),
                cryptographic_binding_methods_supported: vec!["did:jwk".to_string(), "did:key".to_string(),],
                credential_signing_alg_values_supported: vec!["EdDSA".to_string()],
                proof_types_supported: HashMap::from_iter(vec![(
                    ProofType::Jwt,
                    KeyProofMetadata {
                        proof_signing_alg_values_supported: vec![Algorithm::EdDSA],
                    },
                )]),
                display: vec![json!({
                    "locale": "en",
                    "name": "Verifiable Credential",
                    "logo": {
                        "uri": "https://impierce.com/images/logo-blue.png",
                        "alt_text": "UniCore Logo",
                    }
                })],
                ..Default::default()
            };
        pub static ref OPENBADGE_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "id": "https://example.com/credentials/3527",
                "credentialSubject": {
                    "type": [ "AchievementSubject" ],
                    "achievement": {
                              "id": "https://example.com/achievements/21st-century-skills/teamwork",
                              "type": "Achievement",
                              "criteria": {
                                  "narrative": "Team members are nominated for this badge by their peers and recognized upon review by Example Corp management."
                              },
                              "description": "This badge recognizes the development of the capacity to collaborate within a group environment.",
                              "name": "Teamwork"
                          }
                  }
            }
        );
        pub static ref W3C_VC_CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "first_name": "Ferris",
                    "last_name": "Rustacean",
                    "degree": {
                        "type": "MasterDegree",
                        "name": "Master of Oceanography"
                    }
                }
            }
        );
        pub static ref UNSIGNED_OPENBADGE_CREDENTIAL: serde_json::Value = json!({
          "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
          ],
          "id": "https://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://my-domain.example.org/",
            "type": "Profile",
            "name": "UniCore"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "name": "Teamwork Badge",
          "credentialSubject": OPENBADGE_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
        });
        pub static ref UNSIGNED_W3C_VC_CREDENTIAL: serde_json::Value = json!({
          "@context": "https://www.w3.org/2018/credentials/v1",
          "type": [ "VerifiableCredential" ],
          "credentialSubject": W3C_VC_CREDENTIAL_SUBJECT["credentialSubject"].clone(),
          "issuer": {
            "id": "https://my-domain.example.org/",
            "name": "UniCore"
          },
          "issuanceDate": "2010-01-01T00:00:00Z"
        });
    }
}
