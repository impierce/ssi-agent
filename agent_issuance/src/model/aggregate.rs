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
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

// TODO: remove this.
const UNSAFE_ISSUER_KEY: &str = "this-is-a-very-UNSAFE-issuer-key";

fn generate_random_string() -> String {
    let mut rng = rand::thread_rng();

    // Generate 32 random bytes (256 bits)
    let random_bytes: [u8; 32] = rng.gen();

    // Convert the random bytes to a hexadecimal string
    let random_string: String = random_bytes.iter().fold(String::new(), |mut acc, byte| {
        acc.push_str(&format!("{:02x}", byte));
        acc
    });

    random_string
}

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
    pub id: String,
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
            IssuanceCommand::CreateCredentialOffer {
                subject_id,
                pre_authorized_code,
            } => {
                let mut events = vec![];

                let subject = self
                    .subjects
                    .iter()
                    .find(|subject| subject.id == subject_id)
                    .ok_or(MissingIssuanceSubjectError(subject_id.clone()))?;

                if let Some(pre_authorized_code) = pre_authorized_code.as_ref() {
                    events.push(IssuanceEvent::PreAuthorizedCodeUpdated {
                        subject_id: subject_id.clone(),
                        pre_authorized_code: pre_authorized_code.clone(),
                    });
                }

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
                            pre_authorized_code: pre_authorized_code
                                .unwrap_or_else(|| subject.pre_authorized_code.clone()),
                            ..Default::default()
                        }),
                    }),
                });

                events.push(IssuanceEvent::CredentialOfferCreated {
                    subject_id: subject_id.clone(),
                    credential_offer: CredentialOffer {
                        value: credential_offer.clone(),
                        form_urlencoded: credential_offer.to_string(),
                    },
                });

                Ok(events)
            }
            IssuanceCommand::CreateUnsignedCredential { subject_id, credential } => {
                let mut events = vec![];

                if !self.subjects.iter().any(|subject| subject.id == subject_id) {
                    events.push(IssuanceEvent::SubjectCreated {
                        subject: IssuanceSubject {
                            id: subject_id.clone(),
                            pre_authorized_code: generate_random_string(),
                            ..Default::default()
                        },
                    });
                }

                let mut unsigned_credential = self.credential_format_template.clone();

                unsigned_credential
                    .as_object_mut()
                    .ok_or(InvalidCredentialError)?
                    .insert("credentialSubject".to_string(), credential["credentialSubject"].clone());

                events.push(IssuanceEvent::UnsignedCredentialCreated {
                    subject_id,
                    credential: Credential {
                        id: uuid::Uuid::new_v4(),
                        unsigned_credential,
                    },
                });

                Ok(events)
            }
            IssuanceCommand::CreateTokenResponse { token_request } => match token_request {
                TokenRequest::PreAuthorizedCode {
                    pre_authorized_code, ..
                } => {
                    let subject_id = self
                        .subjects
                        .iter()
                        .find(|subject| subject.pre_authorized_code == pre_authorized_code)
                        .map(|subject| subject.id.clone())
                        .ok_or(InvalidPreAuthorizedCodeError)?;

                    #[cfg(test)]
                    let (access_token, c_nonce) = {
                        let access_token = tests::ACCESS_TOKENS.lock().unwrap().pop_front().unwrap();
                        let c_nonce = tests::C_NONCES.lock().unwrap().pop_front().unwrap();
                        (access_token, c_nonce)
                    };
                    #[cfg(not(test))]
                    let (access_token, c_nonce) = { (generate_random_string(), generate_random_string()) };

                    if self.subjects.iter().any(|subject| subject.id == subject_id) {
                        Ok(vec![IssuanceEvent::TokenResponseCreated {
                            subject_id: subject_id.clone(),
                            token_response: TokenResponse {
                                access_token,
                                token_type: "bearer".to_string(),
                                expires_in: None,
                                refresh_token: None,
                                scope: None,
                                c_nonce: Some(c_nonce),
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
                    .map(|subject| subject.id.clone())
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
                    .ok_or(MissingIssuanceSubjectError(subject_id.clone()))?
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
                if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                    subject.credential_offer.replace(credential_offer);
                }
            }
            CredentialFormatTemplateLoaded {
                credential_format_template,
            } => self.credential_format_template = credential_format_template,
            UnsignedCredentialCreated { subject_id, credential } => {
                if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                    subject.credentials.replace(credential);
                }
            }
            PreAuthorizedCodeUpdated {
                subject_id,
                pre_authorized_code,
            } => {
                if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                    subject.pre_authorized_code = pre_authorized_code;
                }
            }
            TokenResponseCreated {
                subject_id,
                token_response,
            } => {
                if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                    subject.token_response.replace(token_response);
                }
            }
            CredentialResponseCreated {
                subject_id,
                credential_response,
            } => {
                if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                    subject.credential_response.replace(credential_response);
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use super::*;
    use agent_shared::UrlAddFunctions;
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
    #[serial_test::serial]
    fn test_create_credentials_supported() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject_1 = subject_1();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
            ])
            .when(IssuanceCommand::CreateCredentialsSupported {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::credentials_supported_created()]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_offer() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject_1 = subject_1();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
                IssuanceEvent::credentials_supported_created(),
            ])
            .when(IssuanceCommand::CreateCredentialOffer {
                subject_id: ISSUANCE_SUBJECT_1.id.clone(),
                pre_authorized_code: Some(PRE_AUTHORIZED_CODE_STRING_1.clone()),
            })
            .then_expect_events(vec![
                IssuanceEvent::pre_authorized_code_updated(subject_1.clone()),
                IssuanceEvent::credential_offer_created(subject_1.clone()),
            ]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_unsigned_credential() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject_1 = subject_1();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::pre_authorized_code_updated(subject_1.clone()),
                IssuanceEvent::credential_offer_created(subject_1.clone()),
            ])
            .when(IssuanceCommand::CreateUnsignedCredential {
                subject_id: ISSUANCE_SUBJECT_1.id.clone(),
                credential: CREDENTIAL_SUBJECT.clone(),
            })
            .then_expect_events(vec![IssuanceEvent::unsigned_credential_created(subject_1.clone())]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_token_response() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject_1 = subject_1();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::pre_authorized_code_updated(subject_1.clone()),
                IssuanceEvent::credential_offer_created(subject_1.clone()),
                IssuanceEvent::unsigned_credential_created(subject_1.clone()),
            ])
            .when(IssuanceCommand::CreateTokenResponse {
                token_request: token_request(subject_1.clone()),
            })
            .then_expect_events(vec![IssuanceEvent::token_response_created(subject_1.clone())]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_response() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject_1 = subject_1();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::pre_authorized_code_updated(subject_1.clone()),
                IssuanceEvent::credential_offer_created(subject_1.clone()),
                IssuanceEvent::unsigned_credential_created(subject_1.clone()),
                IssuanceEvent::token_response_created(subject_1.clone()),
            ])
            .when(IssuanceCommand::CreateCredentialResponse {
                access_token: subject_1.access_token.clone(),
                credential_request: credential_request(subject_1.clone()),
            })
            .then_expect_events(vec![IssuanceEvent::credential_response_created(subject_1.clone())]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_response_with_multiple_subjects() {
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string(), generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string(), generate_random_string()].into();

        let subject_1 = subject_1();
        let subject_2 = subject_2();
        CredentialTestFramework::with(IssuanceServices)
            .given(vec![
                IssuanceEvent::credential_format_template_loaded(),
                IssuanceEvent::authorization_server_metadata_loaded(),
                IssuanceEvent::credential_issuer_metadata_loaded(),
                IssuanceEvent::subject_created(subject_1.clone()),
                IssuanceEvent::subject_created(subject_2.clone()),
                IssuanceEvent::credentials_supported_created(),
                IssuanceEvent::pre_authorized_code_updated(subject_1.clone()),
                IssuanceEvent::pre_authorized_code_updated(subject_2.clone()),
                IssuanceEvent::credential_offer_created(subject_1.clone()),
                IssuanceEvent::credential_offer_created(subject_2.clone()),
                IssuanceEvent::unsigned_credential_created(subject_1.clone()),
                IssuanceEvent::unsigned_credential_created(subject_2.clone()),
                IssuanceEvent::token_response_created(subject_1.clone()),
                IssuanceEvent::token_response_created(subject_2.clone()),
            ])
            .when(IssuanceCommand::CreateCredentialResponse {
                access_token: subject_2.access_token.clone(),
                credential_request: credential_request(subject_2.clone()),
            })
            .then_expect_events(vec![IssuanceEvent::credential_response_created(subject_2.clone())]);
    }

    #[derive(Clone)]
    struct TestSubject {
        issuance_subject: IssuanceSubject,
        key_did: Arc<KeySubject>,
        credential: String,
        access_token: String,
        c_nonce: String,
    }

    lazy_static! {
        pub static ref ACCESS_TOKENS: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref C_NONCES: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        static ref CREDENTIAL_FORMAT_TEMPLATE: serde_json::Value =
            serde_json::from_str(include_str!("../../res/credential_format_templates/openbadges_v3.json")).unwrap();
        static ref BASE_URL: url::Url = "https://example.com/".parse().unwrap();
        static ref AUTHORIZATION_SERVER_METADATA: Box<AuthorizationServerMetadata> =
            Box::new(AuthorizationServerMetadata {
                issuer: BASE_URL.clone(),
                token_endpoint: Some(BASE_URL.add_file("token")),
                ..Default::default()
            });
        static ref CREDENTIAL_ISSUER_METADATA: CredentialIssuerMetadata = CredentialIssuerMetadata {
            credential_issuer: BASE_URL.clone(),
            authorization_server: None,
            credential_endpoint: BASE_URL.add_file("credential"),
            deferred_credential_endpoint: None,
            batch_credential_endpoint: Some(BASE_URL.add_file("batch_credential")),
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
        static ref ISSUANCE_SUBJECT_ID_1: uuid::Uuid = uuid::Uuid::new_v4();
        static ref ISSUANCE_SUBJECT_ID_2: uuid::Uuid = uuid::Uuid::new_v4();
        static ref PRE_AUTHORIZED_CODE_STRING_1: String = generate_random_string();
        static ref PRE_AUTHORIZED_CODE_STRING_2: String = generate_random_string();
        static ref ISSUANCE_SUBJECT_1: IssuanceSubject = IssuanceSubject {
            id: ISSUANCE_SUBJECT_ID_1.to_string(),
            pre_authorized_code: PRE_AUTHORIZED_CODE_STRING_1.clone(),
            ..Default::default()
        };
        static ref ISSUANCE_SUBJECT_2: IssuanceSubject = IssuanceSubject {
            id: ISSUANCE_SUBJECT_ID_2.to_string(),
            pre_authorized_code: PRE_AUTHORIZED_CODE_STRING_2.clone(),
            ..Default::default()
        };
        static ref CREDENTIALS_OBJECTS: Vec<CredentialsObject> = CREDENTIALS_SUPPORTED
            .iter()
            .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
            .collect();
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
        static ref SUBJECT_1_KEY_DID: Arc<KeySubject> = Arc::new(KeySubject::from_keypair(
            from_existing_key::<Ed25519KeyPair>(b"", Some("this-is-a-very-UNSAFE-subj-key-1".as_bytes())),
            None,
        ));
        static ref SUBJECT_2_KEY_DID: Arc<KeySubject> = Arc::new(KeySubject::from_keypair(
            from_existing_key::<Ed25519KeyPair>(b"", Some("this-is-a-very-UNSAFE-subj-key-2".as_bytes())),
            None,
        ));
        static ref VERIFIABLE_CREDENTIAL_JWT_1: String = {
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa3F5WmpEZmhzeVo1YzZOdUpoYm9zV2tTajg2Mmp5V2lDQ\
            0tIRHpOTkttOGtoI3o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCJ9.eyJpc3MiOiJkaWQ6a2V5On\
            o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCIsInN1YiI6ImRpZDprZXk6ejZNa2pNaDdieDNyd25t\
            aWRONzdkYWkxZ2tKWWJSY3J6d1dGOFV1OWtpa2tzMzFmIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbIm\
            h0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3Aw\
            L2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZm\
            lhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rcXlaakRmaHN5WjVjNk51Smhi\
            b3NXa1NqODYyanlXaUNDS0hEek5OS204a2giLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd2\
            9yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rak1oN2J4M3J3bm1pZE43N2RhaTFna0pZYlJjcnp3\
            V0Y4VXU5a2lra3MzMWYiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbG\
            UuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEi\
            OnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY2\
            9nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNv\
            Z25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbn\
            QuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.7hsVlJTwTcZkxI7H0dVjjdtTsmaKE3uLAhLBkavu0eqjQGZWPZqq62tOPVJF_4csi1EvCgeG\
            I5uhrYD2cxM8Bw"
                .to_string()
        };
        static ref VERIFIABLE_CREDENTIAL_JWT_2: String = {
            "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa3F5WmpEZmhzeVo1YzZOdUpoYm9zV2tTajg2Mmp5V2lDQ\
            0tIRHpOTkttOGtoI3o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCJ9.eyJpc3MiOiJkaWQ6a2V5On\
            o2TWtxeVpqRGZoc3laNWM2TnVKaGJvc1drU2o4NjJqeVdpQ0NLSER6Tk5LbThraCIsInN1YiI6ImRpZDprZXk6ejZNa3ZrNVptb2dXa042\
            RmtQRXJaVW1uVlNiQ0tmdEdCVjhqcGJ6UFZmeG8zdERBIiwiZXhwIjo5OTk5OTk5OTk5LCJpYXQiOjAsInZjIjp7IkBjb250ZXh0IjpbIm\
            h0dHBzOi8vd3d3LnczLm9yZy8yMDE4L2NyZWRlbnRpYWxzL3YxIiwiaHR0cHM6Ly9wdXJsLmltc2dsb2JhbC5vcmcvc3BlYy9vYi92M3Aw\
            L2NvbnRleHQtMy4wLjIuanNvbiJdLCJpZCI6Imh0dHA6Ly9leGFtcGxlLmNvbS9jcmVkZW50aWFscy8zNTI3IiwidHlwZSI6WyJWZXJpZm\
            lhYmxlQ3JlZGVudGlhbCIsIk9wZW5CYWRnZUNyZWRlbnRpYWwiXSwiaXNzdWVyIjoiZGlkOmtleTp6Nk1rcXlaakRmaHN5WjVjNk51Smhi\
            b3NXa1NqODYyanlXaUNDS0hEek5OS204a2giLCJpc3N1YW5jZURhdGUiOiIyMDEwLTAxLTAxVDAwOjAwOjAwWiIsIm5hbWUiOiJUZWFtd2\
            9yayBCYWRnZSIsImNyZWRlbnRpYWxTdWJqZWN0Ijp7ImlkIjoiZGlkOmtleTp6Nk1rdms1Wm1vZ1drTjZGa1BFclpVbW5WU2JDS2Z0R0JW\
            OGpwYnpQVmZ4bzN0REEiLCJ0eXBlIjoiQWNoaWV2ZW1lbnRTdWJqZWN0IiwiYWNoaWV2ZW1lbnQiOnsiaWQiOiJodHRwczovL2V4YW1wbG\
            UuY29tL2FjaGlldmVtZW50cy8yMXN0LWNlbnR1cnktc2tpbGxzL3RlYW13b3JrIiwidHlwZSI6IkFjaGlldmVtZW50IiwiY3JpdGVyaWEi\
            OnsibmFycmF0aXZlIjoiVGVhbSBtZW1iZXJzIGFyZSBub21pbmF0ZWQgZm9yIHRoaXMgYmFkZ2UgYnkgdGhlaXIgcGVlcnMgYW5kIHJlY2\
            9nbml6ZWQgdXBvbiByZXZpZXcgYnkgRXhhbXBsZSBDb3JwIG1hbmFnZW1lbnQuIn0sImRlc2NyaXB0aW9uIjoiVGhpcyBiYWRnZSByZWNv\
            Z25pemVzIHRoZSBkZXZlbG9wbWVudCBvZiB0aGUgY2FwYWNpdHkgdG8gY29sbGFib3JhdGUgd2l0aGluIGEgZ3JvdXAgZW52aXJvbm1lbn\
            QuIiwibmFtZSI6IlRlYW13b3JrIn19fX0.X4Hp_bZTfc54c_PcBI2-Qr3YjEZbUB72t4ppo-URRQKDjQXhZmzeox1pMBGQ-hjbNVmdvixw\
            kdSvpwvZnx6zAg"
                .to_string()
        };
    }

    fn subject_1() -> TestSubject {
        TestSubject {
            issuance_subject: ISSUANCE_SUBJECT_1.clone(),
            key_did: SUBJECT_1_KEY_DID.clone(),
            credential: VERIFIABLE_CREDENTIAL_JWT_1.clone(),
            access_token: ACCESS_TOKENS.lock().unwrap()[0].clone(),
            c_nonce: C_NONCES.lock().unwrap()[0].clone(),
        }
    }

    fn subject_2() -> TestSubject {
        TestSubject {
            issuance_subject: ISSUANCE_SUBJECT_2.clone(),
            key_did: SUBJECT_2_KEY_DID.clone(),
            credential: VERIFIABLE_CREDENTIAL_JWT_2.clone(),
            access_token: ACCESS_TOKENS.lock().unwrap()[1].clone(),
            c_nonce: C_NONCES.lock().unwrap()[1].clone(),
        }
    }

    fn token_request(subject: TestSubject) -> TokenRequest {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code: subject.issuance_subject.pre_authorized_code,
            user_pin: None,
        }
    }

    fn token_response(subject: TestSubject) -> TokenResponse {
        TokenResponse {
            access_token: subject.access_token.clone(),
            token_type: "bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
            c_nonce: Some(subject.c_nonce.clone()),
            c_nonce_expires_in: None,
        }
    }

    fn credential_request(subject: TestSubject) -> CredentialRequest {
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
                    .signer(subject.key_did.clone())
                    .iss(subject.key_did.identifier().unwrap())
                    .aud(CREDENTIAL_ISSUER_METADATA.credential_issuer.clone())
                    .iat(1571324800)
                    .exp(9999999999i64)
                    .nonce(subject.c_nonce.clone())
                    .build()
                    .unwrap(),
            ),
        }
    }

    fn credential_response(subject: TestSubject) -> CredentialResponse {
        CredentialResponse {
            credential: CredentialResponseType::Immediate(CredentialFormats::JwtVcJson(
                credential_format_profiles::Credential {
                    format: JwtVcJson,
                    credential: json!(subject.credential.clone()),
                },
            )),
            c_nonce: None,
            c_nonce_expires_in: None,
        }
    }

    impl IssuanceEvent {
        fn credential_format_template_loaded() -> IssuanceEvent {
            IssuanceEvent::CredentialFormatTemplateLoaded {
                credential_format_template: CREDENTIAL_FORMAT_TEMPLATE.clone(),
            }
        }

        fn authorization_server_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::AuthorizationServerMetadataLoaded {
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
            }
        }

        fn credential_issuer_metadata_loaded() -> IssuanceEvent {
            IssuanceEvent::CredentialIssuerMetadataLoaded {
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            }
        }

        fn subject_created(subject: TestSubject) -> IssuanceEvent {
            IssuanceEvent::SubjectCreated {
                subject: subject.issuance_subject,
            }
        }

        fn pre_authorized_code_updated(subject: TestSubject) -> IssuanceEvent {
            IssuanceEvent::PreAuthorizedCodeUpdated {
                subject_id: subject.issuance_subject.id,
                pre_authorized_code: subject.issuance_subject.pre_authorized_code,
            }
        }

        fn credentials_supported_created() -> IssuanceEvent {
            IssuanceEvent::CredentialsSupportedCreated {
                credentials_supported: CREDENTIALS_SUPPORTED.clone(),
            }
        }

        fn credential_offer_created(subject: TestSubject) -> IssuanceEvent {
            let credential_offer_query = CredentialOfferQuery::CredentialOffer(OID4VCICredentialOffer {
                credential_issuer: CREDENTIAL_ISSUER_METADATA.credential_issuer.clone(),
                credentials: CREDENTIALS_OBJECTS.clone(),
                grants: Some(Grants {
                    authorization_code: None,
                    pre_authorized_code: Some(PreAuthorizedCode {
                        pre_authorized_code: subject.issuance_subject.pre_authorized_code,
                        ..Default::default()
                    }),
                }),
            });
            let credential_offer = CredentialOffer {
                value: credential_offer_query.clone(),
                form_urlencoded: credential_offer_query.to_string(),
            };

            IssuanceEvent::CredentialOfferCreated {
                subject_id: subject.issuance_subject.id,
                credential_offer,
            }
        }

        fn unsigned_credential_created(subject: TestSubject) -> IssuanceEvent {
            IssuanceEvent::UnsignedCredentialCreated {
                subject_id: subject.issuance_subject.id,
                credential: CREDENTIAL.clone(),
            }
        }

        fn token_response_created(subject: TestSubject) -> IssuanceEvent {
            IssuanceEvent::TokenResponseCreated {
                subject_id: subject.issuance_subject.id.clone(),
                token_response: token_response(subject),
            }
        }

        fn credential_response_created(subject: TestSubject) -> IssuanceEvent {
            IssuanceEvent::CredentialResponseCreated {
                subject_id: subject.issuance_subject.clone().id,
                credential_response: credential_response(subject),
            }
        }
    }
}
