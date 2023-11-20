use async_trait::async_trait;
use cqrs_es::Aggregate;
use jsonschema::JSONSchema;
use oid4vci::{
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata,
    },
    credential_offer::{CredentialOffer, CredentialsObject, Grants, PreAuthorizedCode},
};
use serde::{Deserialize, Serialize};

use crate::{command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent, services::IssuanceServices};

const UNSAFE_PRE_AUTHORIZED_CODE: &str = "unsafe_pre_authorized_code";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OID4VCIData {
    authorization_server_metadata: Option<AuthorizationServerMetadata>,
    credential_issuer_metadata: Option<CredentialIssuerMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Credential {
    credential_data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssuanceData {
    credential_template: serde_json::Value,
    oid4vci_data: OID4VCIData,
    credential: Credential,
    // TODO: add proof?
    // proof: Option<T>
}

#[async_trait]
impl Aggregate for IssuanceData {
    type Command = IssuanceCommand;
    type Event = IssuanceEvent;
    type Error = IssuanceError;
    type Services = IssuanceServices;

    fn aggregate_type() -> String {
        "Credential".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            IssuanceCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata,
            } => Ok(vec![IssuanceEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            }]),
            IssuanceCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata,
            } => Ok(vec![IssuanceEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            }]),
            IssuanceCommand::CreateCredentialsSupported { credentials_supported } => {
                self.oid4vci_data
                    .credential_issuer_metadata
                    .as_ref()
                    .ok_or(IssuanceError::from("Missing Credential Issuer Metadata"))?;
                Ok(vec![IssuanceEvent::CredentialsSupportedCreated {
                    credentials_supported,
                }])
            }
            IssuanceCommand::CreateCredentialOffer => {
                let credential_issuer_metadata =
                    self.oid4vci_data
                        .credential_issuer_metadata
                        .as_ref()
                        .ok_or(IssuanceError::from(
                            "Missing Credential Issuer Metadata. Please load it before creating offer.",
                        ))?;
                let credentials_supported = credential_issuer_metadata.credentials_supported.clone();
                let credential_offer = CredentialOffer {
                    credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
                    credentials: credentials_supported
                        .iter()
                        .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
                        .collect(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
                            ..Default::default()
                        }),
                    }),
                };

                Ok(vec![IssuanceEvent::CredentialOfferCreated { credential_offer }])
            }
            IssuanceCommand::LoadCredentialTemplate { credential_template } => {
                JSONSchema::compile(&credential_template).map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                Ok(vec![IssuanceEvent::CredentialTemplateLoaded { credential_template }])
            }
            IssuanceCommand::CreateCredentialData { credential } => {
                let credential_template = self.credential_template.clone();
                let json_schema = JSONSchema::compile(&credential_template)
                    .map_err(|e| IssuanceError::from(e.to_string().as_str()))?;

                json_schema.validate(&credential).map_err(|e| {
                    // TODO: remove ugly solution.
                    let e: Vec<_> = e.map(|e| e.to_string()).collect();
                    IssuanceError::from(e.join(", ").as_str())
                })?;

                Ok(vec![IssuanceEvent::CredentialDataCreated {
                    credential_template,
                    credential_data: credential,
                }])
            }
            _ => unimplemented!(),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use IssuanceEvent::*;
        match event {
            AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            } => self.oid4vci_data.authorization_server_metadata = Some(authorization_server_metadata),
            CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            } => self.oid4vci_data.credential_issuer_metadata = Some(credential_issuer_metadata),
            CredentialsSupportedCreated { credentials_supported } => {
                self.oid4vci_data
                    .credential_issuer_metadata
                    .as_mut()
                    .unwrap()
                    .credentials_supported = credentials_supported
            }
            CredentialOfferCreated { .. } => todo!(),
            CredentialTemplateLoaded { credential_template } => self.credential_template = credential_template,
            CredentialDataCreated {
                credential_template,
                credential_data,
            } => {
                self.credential_template = credential_template;
                self.credential.credential_data = credential_data;
            }
            CredentialSigned { .. } => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cqrs_es::test::TestFramework;
    use lazy_static::lazy_static;
    use oid4vci::{
        credential_issuer::credentials_supported::CredentialsSupportedObject,
        credential_offer::{CredentialOffer, CredentialsObject, Grants, PreAuthorizedCode},
    };
    use serde_json::json;

    type CredentialTestFramework = TestFramework<IssuanceData>;

    lazy_static! {
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
        static ref AUTHORIZATION_SERVER_METADATA: AuthorizationServerMetadata = AuthorizationServerMetadata {
            issuer: BASE_URL.clone(),
            token_endpoint: Some(BASE_URL.join("token").unwrap()),
            ..Default::default()
        };
        static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: BASE_URL.clone(),
            authorization_server: None,
            credential_endpoint: BASE_URL.join("credential").unwrap(),
            deferred_credential_endpoint: None,
            batch_credential_endpoint: Some(BASE_URL.join("batch_credential").unwrap()),
            credentials_supported: vec![],
            display: None,
        };
        static ref CREDENTIALS_SUPPORTED: Vec<CredentialsSupportedObject> = vec![serde_json::from_value(json!({
            "format": "jwt_vc_json",
            "cryptographic_binding_methods_supported": [
                "did:key",
            ],
            "cryptographic_suites_supported": [
                "EdDSA"
            ],
            "credential_definition":{
                "type": [
                    "VerifiableCredential",
                    "OpenBadgeCredential"
                ]
            },
            "proof_types_supported": [
                "jwt"
            ]
        }
        ))
        .unwrap()];
        static ref CREDENTIALS_OBJECTS: Vec<CredentialsObject> = CREDENTIALS_SUPPORTED
            .iter()
            .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
            .collect();
        pub static ref PRE_AUTHORIZED_CODE: PreAuthorizedCode = PreAuthorizedCode {
            pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
            ..Default::default()
        };
    }

    pub fn credential_template() -> serde_json::Value {
        serde_json::from_str(include_str!("../../res/json_schema/openbadges_v3.json")).unwrap()
    }

    impl IssuanceEvent {
        pub fn authorization_server_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone().clone(),
            }
        }

        pub fn credential_issuer_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }
        }

        pub fn credentials_supported_created() -> IssuanceEvent {
            IssuanceEvent::CredentialsSupportedCreated {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            }
        }

        pub fn credential_offer_created() -> IssuanceEvent {
            IssuanceEvent::CredentialOfferCreated {
                credential_offer: CredentialOffer {
                    credential_issuer: CREDENTIAL_ISSUER_METADATA.credential_issuer.clone(),
                    credentials: CREDENTIALS_OBJECTS.clone(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PRE_AUTHORIZED_CODE.clone()),
                    }),
                },
            }
        }
    }

    #[test]
    fn test_authorization_server_metadata_loaded() {
        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::authorization_server_metadata_loaded()]);
    }

    #[test]
    fn test_credential_issuer_metadata_loaded() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::authorization_server_metadata_loaded()])
            .when(IssuanceCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credential_issuer_metadata_loaded()]);
    }

    #[test]
    fn test_credentials_supported_created() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
            ])
            .when(IssuanceCommand::CreateCredentialsSupported {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credentials_supported_created()]);
    }

    #[test]
    fn test_credential_offer_created() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::credentials_supported_created(),
            ])
            .when(IssuanceCommand::CreateCredentialOffer)
            .then_expect_events(vec![IssuanceEvent::credential_offer_created()]);
    }

    #[test]
    fn test_credential_template_loaded() {
        let expected = IssuanceEvent::CredentialTemplateLoaded {
            credential_template: credential_template(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadCredentialTemplate {
                credential_template: credential_template(),
            })
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_create_data_created() {
        let credential = json!({
          "@context": [
            "https://www.w3.org/2018/credentials/v1",
            "https://purl.imsglobal.org/spec/ob/v3p0/context-3.0.2.json"
          ],
          "id": "http://example.com/credentials/3527",
          "type": ["VerifiableCredential", "OpenBadgeCredential"],
          "issuer": {
            "id": "https://example.com/issuers/876543",
            "type": "Profile",
            "name": "Example Corp"
          },
          "issuanceDate": "2010-01-01T00:00:00Z",
          "name": "Teamwork Badge",
          "credentialSubject": {
            "id": "did:example:ebfeb1f712ebc6f1c276e12ec21",
            "type": "AchievementSubject",
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
        });

        let expected = IssuanceEvent::CredentialDataCreated {
            credential_template: credential_template(),
            credential_data: credential.clone(),
        };

        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::CredentialTemplateLoaded {
                credential_template: credential_template(),
            }])
            .when(IssuanceCommand::CreateCredentialData { credential })
            .then_expect_events(vec![expected]);
    }
}
