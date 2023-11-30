use crate::{command::IssuanceCommand, error::IssuanceError, event::IssuanceEvent, services::IssuanceServices};
use async_trait::async_trait;
use cqrs_es::Aggregate;
use derivative::Derivative;
use did_key::{from_existing_key, Ed25519KeyPair};
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::{jwt, Decoder, Subjects};
use oid4vc_manager::methods::key_method::KeySubject;
use oid4vci::{
    credential_format_profiles::{self, w3c_verifiable_credentials::jwt_vc_json::JwtVcJson, CredentialFormats},
    credential_issuer::{
        authorization_server_metadata::AuthorizationServerMetadata,
        credential_issuer_metadata::CredentialIssuerMetadata, CredentialIssuer,
    },
    credential_offer::{
        CredentialOffer as OID4VCICredentialOffer, CredentialOfferQuery, CredentialsObject, Grants, PreAuthorizedCode,
    },
    credential_response::{CredentialResponse, CredentialResponseType},
    token_request::TokenRequest,
    token_response::TokenResponse,
    VerifiableCredentialJwt,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

// TODO: remove this.
const UNSAFE_ACCESS_TOKEN: &str = "unsafe_access_token";
const UNSAFE_C_NONCE: &str = "unsafe_c_nonce";
const UNSAFE_ISSUER_KEY: &str = "this-is-a-very-UNSAFE-issuer-key";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OID4VCIData {
    pub authorization_server_metadata: Option<AuthorizationServerMetadata>,
    pub credential_issuer_metadata: Option<CredentialIssuerMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct Credential {
    #[derivative(PartialEq = "ignore")]
    id: uuid::Uuid,
    pub unsigned_credential: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CredentialOffer {
    value: CredentialOfferQuery,
    pub form_urlencoded: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Derivative)]
#[derivative(PartialEq)]
pub struct IssuanceSubject {
    #[derivative(PartialEq = "ignore")]
    pub id: uuid::Uuid,
    pub credential_offer: Option<CredentialOffer>,
    pub credentials: Option<Credential>,
    pub pre_authorized_code: String,
    pub token_response: Option<TokenResponse>,
    pub credential_response: Option<CredentialResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IssuanceData {
    credential_format_template: serde_json::Value,
    oid4vci_data: OID4VCIData,
    subjects: Vec<IssuanceSubject>,
}

#[async_trait]
impl Aggregate for IssuanceData {
    type Command = IssuanceCommand;
    type Event = IssuanceEvent;
    type Error = IssuanceError;
    type Services = IssuanceServices;

    fn aggregate_type() -> String {
        "IssuanceData".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        use IssuanceError::*;

        match command {
            IssuanceCommand::LoadCredentialFormatTemplate {
                credential_format_template,
            } => Ok(vec![IssuanceEvent::CredentialFormatTemplateLoaded {
                credential_format_template,
            }]),
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
                    .ok_or(MissingCredentialIssuerMetadataError)?;
                Ok(vec![IssuanceEvent::CredentialsSupportedCreated {
                    credentials_supported,
                }])
            }
            IssuanceCommand::CreateSubject { pre_authorized_code } => {
                let subject = IssuanceSubject {
                    id: uuid::Uuid::new_v4(),
                    pre_authorized_code,
                    ..Default::default()
                };
                Ok(vec![IssuanceEvent::SubjectCreated { subject }])
            }
            IssuanceCommand::CreateCredentialOffer { subject_id } => {
                let subject = self
                    .subjects
                    .iter()
                    .find(|subject| subject.id == subject_id)
                    .ok_or(MissingIssuanceSubjectError(subject_id))?;

                let credential_issuer_metadata = self
                    .oid4vci_data
                    .credential_issuer_metadata
                    .as_ref()
                    .ok_or(MissingCredentialIssuerMetadataError)?;
                let credentials_supported = credential_issuer_metadata.credentials_supported.clone();
                let credential_offer = CredentialOfferQuery::CredentialOffer(OID4VCICredentialOffer {
                    credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
                    credentials: credentials_supported
                        .iter()
                        .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
                        .collect(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: subject.pre_authorized_code.clone(),
                            ..Default::default()
                        }),
                    }),
                });

                Ok(vec![IssuanceEvent::CredentialOfferCreated {
                    subject_id,
                    credential_offer: CredentialOffer {
                        value: credential_offer.clone(),
                        form_urlencoded: credential_offer.to_string(),
                    },
                }])
            }

            IssuanceCommand::CreateUnsignedCredential { subject_id, credential } => {
                let mut unsigned_credential = self.credential_format_template.clone();

                unsigned_credential
                    .as_object_mut()
                    .ok_or(InvalidCredentialError)?
                    .insert("credentialSubject".to_string(), credential["credentialSubject"].clone());

                Ok(vec![IssuanceEvent::UnsignedCredentialCreated {
                    subject_id,
                    credential: Credential {
                        id: uuid::Uuid::new_v4(),
                        unsigned_credential,
                    },
                }])
            }
            IssuanceCommand::CreateTokenResponse { token_request } => match token_request {
                TokenRequest::PreAuthorizedCode {
                    pre_authorized_code, ..
                } => {
                    let subject_id = self
                        .subjects
                        .iter()
                        .find(|subject| subject.pre_authorized_code == pre_authorized_code)
                        .map(|subject| subject.id)
                        .ok_or(InvalidPreAuthorizedCodeError)?;

                    if self.subjects.iter().find(|subject| subject.id == subject_id).is_some() {
                        Ok(vec![IssuanceEvent::TokenResponseCreated {
                            subject_id: subject_id.clone(),
                            token_response: TokenResponse {
                                access_token: UNSAFE_ACCESS_TOKEN.to_string(),
                                token_type: "bearer".to_string(),
                                expires_in: None,
                                refresh_token: None,
                                scope: None,
                                c_nonce: Some(UNSAFE_C_NONCE.to_string()),
                                c_nonce_expires_in: None,
                            },
                        }])
                    } else {
                        Err(InvalidPreAuthorizedCodeError)
                    }
                }
                _ => Err(InvalidPreAuthorizedCodeError),
            },
            IssuanceCommand::CreateCredentialResponse {
                access_token,
                credential_request,
            } => {
                use oid4vc_core::Subject;

                let subject_id = self
                    .subjects
                    .iter()
                    .find(|subject| {
                        subject
                            .token_response
                            .as_ref()
                            .map_or(false, |res| res.access_token == access_token)
                    })
                    .map(|subject| subject.id)
                    .ok_or(InvalidAccessTokenError)?;

                // TODO: utilize `agent_kms`.
                let issuer = Arc::new(KeySubject::from_keypair(
                    from_existing_key::<Ed25519KeyPair>(b"", Some(UNSAFE_ISSUER_KEY.as_bytes())),
                    None,
                ));
                let issuer_did = issuer.identifier().unwrap();

                let credential_issuer = CredentialIssuer {
                    subject: issuer.clone(),
                    metadata: self
                        .oid4vci_data
                        .credential_issuer_metadata
                        .as_ref()
                        .ok_or(MissingCredentialIssuerMetadataError)?
                        .clone(),
                    authorization_server_metadata: self
                        .oid4vci_data
                        .authorization_server_metadata
                        .as_ref()
                        .ok_or(MissingAuthorizationServerMetadataError)?
                        .clone(),
                };

                let proof = credential_issuer
                    .validate_proof(
                        credential_request.proof.ok_or(MissingProofError)?,
                        Decoder::from(&Subjects::try_from([issuer.clone() as Arc<dyn Subject>]).unwrap()),
                    )
                    .await
                    .map_err(|_| InvalidProofError)?;

                let subject_did = proof
                    .rfc7519_claims
                    .iss()
                    .as_ref()
                    .ok_or(MissingProofIssuerError)?
                    .clone();

                let mut credential = self
                    .subjects
                    .iter()
                    .find(|subject| subject.id == subject_id)
                    .ok_or(MissingIssuanceSubjectError(subject_id))?
                    .credentials
                    .as_ref()
                    .ok_or(MissingCredentialError)?
                    .unsigned_credential
                    .clone();

                credential["issuer"] = json!(issuer_did);
                credential["credentialSubject"]["id"] = json!(subject_did);
                let credential_response = CredentialResponse {
                    credential: CredentialResponseType::Immediate(CredentialFormats::JwtVcJson(
                        credential_format_profiles::Credential {
                            format: JwtVcJson,
                            credential: json!(jwt::encode(
                                issuer.clone(),
                                Header::new(Algorithm::EdDSA),
                                VerifiableCredentialJwt::builder()
                                    .sub(subject_did)
                                    .iss(issuer_did)
                                    .iat(0)
                                    .exp(9999999999i64)
                                    .verifiable_credential(credential)
                                    .build()
                                    .ok(),
                            )
                            .ok()),
                        },
                    )),
                    c_nonce: None,
                    c_nonce_expires_in: None,
                };

                Ok(vec![IssuanceEvent::CredentialResponseCreated {
                    subject_id,
                    credential_response,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use IssuanceEvent::*;
        match event {
            AuthorizationServerMetadataLoaded {
                authorization_server_metadata,
            } => {
                self.oid4vci_data
                    .authorization_server_metadata
                    .replace(*authorization_server_metadata);
            }
            CredentialIssuerMetadataLoaded {
                credential_issuer_metadata,
            } => {
                self.oid4vci_data
                    .credential_issuer_metadata
                    .replace(credential_issuer_metadata);
            }
            CredentialsSupportedCreated { credentials_supported } => {
                self.oid4vci_data
                    .credential_issuer_metadata
                    .as_mut()
                    .unwrap()
                    .credentials_supported = credentials_supported
            }
            SubjectCreated { subject } => self.subjects.push(subject),
            CredentialOfferCreated {
                subject_id,
                credential_offer,
            } => {
                self.subjects
                    .iter_mut()
                    .find(|subject| subject.id == subject_id)
                    .map(|subject| {
                        subject.credential_offer.replace(credential_offer);
                    });
            }
            CredentialFormatTemplateLoaded {
                credential_format_template,
            } => self.credential_format_template = credential_format_template,
            UnsignedCredentialCreated { subject_id, credential } => {
                self.subjects
                    .iter_mut()
                    .find(|subject| subject.id == subject_id)
                    .map(|subject| {
                        subject.credentials.replace(credential);
                    });
            }
            TokenResponseCreated {
                subject_id,
                token_response,
            } => {
                self.subjects
                    .iter_mut()
                    .find(|subject| subject.id == subject_id)
                    .map(|subject| {
                        subject.token_response.replace(token_response);
                    });
            }
            CredentialResponseCreated {
                subject_id,
                credential_response,
            } => {
                self.subjects
                    .iter_mut()
                    .find(|subject| subject.id == subject_id)
                    .map(|subject| {
                        subject.credential_response.replace(credential_response);
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use cqrs_es::test::TestFramework;
    use did_key::Ed25519KeyPair;
    use lazy_static::lazy_static;
    use oid4vc_manager::methods::key_method::KeySubject;
    use oid4vci::{
        credential_format_profiles::{
            self,
            w3c_verifiable_credentials::jwt_vc_json::{CredentialDefinition, JwtVcJson},
            CredentialFormats, Parameters,
        },
        credential_issuer::credentials_supported::CredentialsSupportedObject,
        credential_offer::{CredentialsObject, Grants, PreAuthorizedCode},
        credential_request::CredentialRequest,
        credential_response::{CredentialResponse, CredentialResponseType},
        token_request::TokenRequest,
        token_response::TokenResponse,
        Proof, ProofType,
    };
    use serde_json::json;

    type CredentialTestFramework = TestFramework<IssuanceData>;

    #[test]
    fn test_load_credential_format_template() {
        CredentialTestFramework::with(IssuanceServices)
            .given_no_previous_events()
            .when(IssuanceCommand::LoadCredentialFormatTemplate {
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credential_format_template_loaded()]);
    }

    #[test]
    fn test_load_authorization_server_metadata() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![IssuanceEvent::credential_format_template_loaded()])
            .when(IssuanceCommand::LoadAuthorizationServerMetadata {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::authorization_server_metadata_loaded()]);
    }

    #[test]
    fn test_load_credential_issuer_metadata() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
            ])
            .when(IssuanceCommand::LoadCredentialIssuerMetadata {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credential_issuer_metadata_loaded()]);
    }

    #[test]
    fn test_create_subject() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
            ])
            .when(IssuanceCommand::CreateSubject {
                pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
            })
            .then_expect_events(vec![IssuanceEvent::subject_created()]);
    }

    #[test]
    fn test_create_credentials_supported() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(),
            ])
            .when(IssuanceCommand::CreateCredentialsSupported {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credentials_supported_created()]);
    }

    #[test]
    fn test_create_credential_offer() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(),
                IssuanceEvent::credentials_supported_created(),
            ])
            .when(IssuanceCommand::CreateCredentialOffer {
                subject_id: ISSUANCE_SUBJECT.id,
            })
            .then_expect_events(vec![IssuanceEvent::credential_offer_created()]);
    }

    #[test]
    fn test_create_unsigned_credential() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::credential_offer_created(),
            ])
            .when(IssuanceCommand::CreateUnsignedCredential {
                subject_id: ISSUANCE_SUBJECT.id,
                credential: CREDENTIAL_SUBJECT.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::unsigned_credential_created()]);
    }

    #[test]
    fn test_create_token_response() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::credential_offer_created(),
                IssuanceEvent::unsigned_credential_created(),
            ])
            .when(IssuanceCommand::CreateTokenResponse {
                token_request: TOKEN_REQUEST.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::token_response_created()]);
    }

    #[test]
    fn test_create_credential_response() {
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::credential_offer_created(),
                IssuanceEvent::unsigned_credential_created(),
                IssuanceEvent::token_response_created(),
            ])
            .when(IssuanceCommand::CreateCredentialResponse {
                access_token: UNSAFE_ACCESS_TOKEN.to_string(),
                credential_request: CREDENTIAL_REQUEST.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credential_response_created()]);
    }

    const UNSAFE_PRE_AUTHORIZED_CODE: &str = "unsafe_pre_authorized_code";

    lazy_static! {
        static ref CREDENTIAL_FORMAT_TEMPLATE: serde_json::Value =
            serde_json::from_str(include_str!("../../res/credential_format_templates/openbadges_v3.json")).unwrap();
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
        static ref AUTHORIZATION_SERVER_METADATA: Box<AuthorizationServerMetadata> =
            Box::new(AuthorizationServerMetadata {
                issuer: BASE_URL.clone(),
                token_endpoint: Some(BASE_URL.join("token").unwrap()),
                ..Default::default()
            });
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
        static ref ISSUANCE_SUBJECT_ID: uuid::Uuid = uuid::Uuid::new_v4();
        static ref ISSUANCE_SUBJECT: IssuanceSubject = IssuanceSubject {
            id: ISSUANCE_SUBJECT_ID.clone(),
            pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
            ..Default::default()
        };
        static ref CREDENTIALS_OBJECTS: Vec<CredentialsObject> = CREDENTIALS_SUPPORTED
            .iter()
            .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
            .collect();
        static ref PRE_AUTHORIZED_CODE: PreAuthorizedCode = PreAuthorizedCode {
            pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
            ..Default::default()
        };
        static ref CREDENTIAL_OFFER: CredentialOffer = {
            let credential_offer = CredentialOfferQuery::CredentialOffer(OID4VCICredentialOffer {
                credential_issuer: CREDENTIAL_ISSUER_METADATA.credential_issuer.clone(),
                credentials: CREDENTIALS_OBJECTS.clone(),
                grants: Some(Grants {
                    authorization_code: None,
                    pre_authorized_code: Some(PRE_AUTHORIZED_CODE.clone()),
                }),
            });
            CredentialOffer {
                value: credential_offer.clone(),
                form_urlencoded: credential_offer.to_string(),
            }
        };
        static ref CREDENTIAL_SUBJECT: serde_json::Value = json!(
            {
                "credentialSubject": {
                    "id": {},
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
            }
        );
        static ref UNSIGNED_CREDENTIAL: serde_json::Value = json!({
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
          "credentialSubject": CREDENTIAL_SUBJECT["credentialSubject"].clone(),
        });
        static ref CREDENTIAL: Credential = Credential {
            id: uuid::Uuid::new_v4(),
            unsigned_credential: UNSIGNED_CREDENTIAL.clone(),
        };
        static ref TOKEN_REQUEST: TokenRequest = TokenRequest::PreAuthorizedCode {
            pre_authorized_code: UNSAFE_PRE_AUTHORIZED_CODE.to_string(),
            user_pin: None,
        };
        static ref TOKEN_RESPONSE: TokenResponse = TokenResponse {
            access_token: UNSAFE_ACCESS_TOKEN.to_string(),
            token_type: "bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
            c_nonce: Some(UNSAFE_C_NONCE.to_string()),
            c_nonce_expires_in: None,
        };
        static ref SUBJECT_KEY_DID: Arc<KeySubject> = Arc::new(KeySubject::from_keypair(
            from_existing_key::<Ed25519KeyPair>(
                b"",
                Some("this-is-a-very-UNSAFE-subjec-key".as_bytes().try_into().unwrap(),),
            ),
            None,
        ));
        static ref CREDENTIAL_REQUEST: CredentialRequest = {
            use oid4vc_core::Subject;

            CredentialRequest {
                credential_format: CredentialFormats::JwtVcJson(Parameters {
                    format: JwtVcJson,
                    parameters: (
                        CredentialDefinition {
                            type_: vec!["VerifiableCredential".to_string(), "OpenBadgeCredential".to_string()],
                            credential_subject: None,
                        },
                        None,
                    )
                        .into(),
                }),
                proof: Some(
                    Proof::builder()
                        .proof_type(ProofType::Jwt)
                        .signer(SUBJECT_KEY_DID.clone())
                        .iss(SUBJECT_KEY_DID.identifier().unwrap())
                        .aud(CREDENTIAL_ISSUER_METADATA.credential_issuer.clone())
                        .iat(1571324800)
                        .exp(9999999999i64)
                        .nonce(UNSAFE_C_NONCE.to_string())
                        .build()
                        .unwrap(),
                ),
            }
        };
        static ref VERIFIABLE_CREDENTIAL_JWT: String = {
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa3F5WmpEZmhzeVo1YzZOdUpoYm9zV2tTajg2Mmp5V2lDQ0tIRHpOTkttOGtoI3o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCIsInN1YiI6ImRpZDprZXk6ejZNa3RRcEJuTDY5MVdmdGhRV0xTNk0zQjZhczh2akx1Z29kZVFhWGtKVHdyclNxIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbImh0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3AwL2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZmlhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rcXlaakRmaHN5WjVjNk51Smhib3NXa1NqODYyanlXaUNDS0hEek5OS204a2giLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd29yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rdFFwQm5MNjkxV2Z0aFFXTFM2TTNCNmFzOHZqTHVnb2RlUWFYa0pUd3JyU3EiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbGUuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEiOnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY29nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNvZ25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbnQuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.Klwqycvwq86HbwaRxO4kwNkgrComMTbQhAHrU5j9dNKC0IQg3-KB8SDVxbbOSCnta7yDGSkq45M931XmIPYuCg".to_string()
        };
        static ref CREDENTIAL_RESPONSE: CredentialResponse = CredentialResponse {
            credential: CredentialResponseType::Immediate(CredentialFormats::JwtVcJson(
                credential_format_profiles::Credential {
                    format: JwtVcJson,
                    credential: json!(VERIFIABLE_CREDENTIAL_JWT.clone()),
                }
            )),
            c_nonce: None,
            c_nonce_expires_in: None,
        };
    }

    impl IssuanceEvent {
        pub fn credential_format_template_loaded() -> IssuanceEvent {
            IssuanceEvent::CredentialFormatTemplateLoaded {
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            }
        }

        pub fn authorization_server_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
            }
        }

        pub fn credential_issuer_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }
        }

        pub fn subject_created() -> IssuanceEvent {
            IssuanceEvent::SubjectCreated {
                subject: ISSUANCE_SUBJECT.clone(),
            }
        }

        pub fn credentials_supported_created() -> IssuanceEvent {
            IssuanceEvent::CredentialsSupportedCreated {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            }
        }

        pub fn credential_offer_created() -> IssuanceEvent {
            IssuanceEvent::CredentialOfferCreated {
                subject_id: ISSUANCE_SUBJECT.id,
                credential_offer: CREDENTIAL_OFFER.clone(),
            }
        }

        pub fn unsigned_credential_created() -> IssuanceEvent {
            IssuanceEvent::UnsignedCredentialCreated {
                subject_id: ISSUANCE_SUBJECT.id,
                credential: CREDENTIAL.clone(),
            }
        }

        pub fn token_response_created() -> IssuanceEvent {
            IssuanceEvent::TokenResponseCreated {
                subject_id: ISSUANCE_SUBJECT.id,
                token_response: TOKEN_RESPONSE.clone(),
            }
        }

        pub fn credential_response_created() -> IssuanceEvent {
            IssuanceEvent::CredentialResponseCreated {
                subject_id: ISSUANCE_SUBJECT.id,
                credential_response: CREDENTIAL_RESPONSE.clone(),
            }
        }
    }
}
