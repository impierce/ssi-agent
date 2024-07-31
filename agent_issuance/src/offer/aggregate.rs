use agent_shared::generate_random_string;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::Validator;
use oid4vci::credential_issuer::CredentialIssuer;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseType};
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError::{self, *};
use crate::offer::event::OfferEvent;
use crate::services::IssuanceServices;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Offer {
    pub subject_id: Option<String>,
    pub credential_ids: Vec<String>,
    pub form_url_encoded_credential_offer: String,
    pub pre_authorized_code: String,
    pub token_response: Option<TokenResponse>,
    pub access_token: String,
    pub credential_response: Option<CredentialResponse>,
}

#[async_trait]
impl Aggregate for Offer {
    type Command = OfferCommand;
    type Event = OfferEvent;
    type Error = OfferError;
    type Services = Arc<IssuanceServices>;

    fn aggregate_type() -> String {
        "offer".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use OfferCommand::*;
        use OfferEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreateCredentialOffer { offer_id } => {
                #[cfg(test)]
                let (pre_authorized_code, access_token) = {
                    let pre_authorized_code = tests::PRE_AUTHORIZED_CODES.lock().unwrap().pop_front().unwrap();
                    let access_token = tests::ACCESS_TOKENS.lock().unwrap().pop_front().unwrap();
                    (pre_authorized_code, access_token)
                };
                #[cfg(not(test))]
                let (pre_authorized_code, access_token) = { (generate_random_string(), generate_random_string()) };

                Ok(vec![CredentialOfferCreated {
                    offer_id,
                    pre_authorized_code,
                    access_token,
                }])
            }
            AddCredentials {
                offer_id,
                credential_ids,
            } => Ok(vec![CredentialsAdded {
                offer_id,
                credential_ids,
            }]),
            CreateFormUrlEncodedCredentialOffer {
                offer_id,
                credential_issuer_metadata,
            } => {
                // TODO: This needs to be fixed when we implement Batch credentials.
                let credentials_supported = credential_issuer_metadata.credential_configurations_supported.clone();
                let credential_offer = CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
                    credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
                    credential_configuration_ids: credentials_supported.keys().cloned().collect(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: self.pre_authorized_code.clone(),
                            ..Default::default()
                        }),
                    }),
                }));
                Ok(vec![FormUrlEncodedCredentialOfferCreated {
                    offer_id,
                    form_url_encoded_credential_offer: credential_offer.to_string(),
                }])
            }
            CreateTokenResponse {
                offer_id,
                token_request,
            } => {
                #[cfg(test)]
                let c_nonce = tests::C_NONCES.lock().unwrap().pop_front().unwrap();
                #[cfg(not(test))]
                let c_nonce = generate_random_string();

                match token_request {
                    TokenRequest::PreAuthorizedCode { .. } => Ok(vec![TokenResponseCreated {
                        offer_id,
                        token_response: TokenResponse {
                            access_token: self.access_token.clone(),
                            token_type: "bearer".to_string(),
                            expires_in: None,
                            refresh_token: None,
                            scope: None,
                            c_nonce: Some(c_nonce),
                            c_nonce_expires_in: None,
                        },
                    }]),
                    _ => todo!(),
                }
            }
            VerifyCredentialRequest {
                offer_id,
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_request,
            } => {
                let credential_issuer = CredentialIssuer {
                    subject: services.issuer.clone(),
                    metadata: credential_issuer_metadata,
                    authorization_server_metadata: *authorization_server_metadata,
                };

                let proof = credential_issuer
                    .validate_proof(
                        credential_request.proof.ok_or(MissingProofError)?,
                        Validator::Subject(services.issuer.clone()),
                    )
                    .await
                    .map_err(|e| InvalidProofError(e.to_string()))?;

                let subject_did = proof
                    .rfc7519_claims
                    .iss()
                    .as_ref()
                    .ok_or(MissingProofIssuerError)?
                    .clone();

                Ok(vec![CredentialRequestVerified {
                    offer_id,
                    subject_id: subject_did,
                }])
            }
            CreateCredentialResponse {
                offer_id,
                mut signed_credentials,
            } => {
                // TODO: support batch credentials.
                let signed_credential = signed_credentials.pop().ok_or(MissingCredentialError)?;

                let credential_response = CredentialResponse {
                    credential: CredentialResponseType::Immediate {
                        credential: signed_credential,
                        notification_id: None,
                    },
                    c_nonce: None,
                    c_nonce_expires_in: None,
                };

                Ok(vec![CredentialResponseCreated {
                    offer_id,
                    credential_response,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            CredentialOfferCreated {
                pre_authorized_code,
                access_token,
                ..
            } => {
                self.pre_authorized_code = pre_authorized_code;
                self.access_token = access_token;
            }
            CredentialsAdded { credential_ids, .. } => {
                self.credential_ids = credential_ids;
            }
            FormUrlEncodedCredentialOfferCreated {
                form_url_encoded_credential_offer,
                ..
            } => {
                self.form_url_encoded_credential_offer = form_url_encoded_credential_offer;
            }
            CredentialRequestVerified { subject_id, .. } => {
                self.subject_id.replace(subject_id);
            }
            TokenResponseCreated { token_response, .. } => {
                self.token_response.replace(token_response);
            }
            CredentialResponseCreated {
                credential_response, ..
            } => {
                self.credential_response.replace(credential_response);
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    use cqrs_es::test::TestFramework;
    use jsonwebtoken::Algorithm;
    use lazy_static::lazy_static;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_request::CredentialRequest,
        KeyProofType, ProofType,
    };
    use rstest::rstest;
    use serde_json::json;
    use std::{collections::VecDeque, sync::Mutex};

    use crate::{
        credential::aggregate::credential_tests::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT,
        server_config::aggregate::server_config_tests::{AUTHORIZATION_SERVER_METADATA, CREDENTIAL_ISSUER_METADATA},
        services::test_utils::test_issuance_services,
    };

    type OfferTestFramework = TestFramework<Offer>;

    #[test]
    #[serial_test::serial]
    fn test_create_offer() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer {
                offer_id: Default::default(),
            })
            .then_expect_events(vec![OfferEvent::CredentialOfferCreated {
                offer_id: Default::default(),
                pre_authorized_code: subject.pre_authorized_code,
                access_token: subject.access_token,
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_add_credential() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given(vec![OfferEvent::CredentialOfferCreated {
                offer_id: Default::default(),
                pre_authorized_code: subject.pre_authorized_code.clone(),
                access_token: subject.access_token.clone(),
            }])
            .when(OfferCommand::AddCredentials {
                offer_id: Default::default(),
                credential_ids: vec!["credential-id".to_string()],
            })
            .then_expect_events(vec![OfferEvent::CredentialsAdded {
                offer_id: Default::default(),
                credential_ids: vec!["credential-id".to_string()],
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_credential_offer() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    pre_authorized_code: subject.pre_authorized_code,
                    access_token: subject.access_token,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
            ])
            .when(OfferCommand::CreateFormUrlEncodedCredentialOffer {
                offer_id: Default::default(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
            })
            .then_expect_events(vec![OfferEvent::FormUrlEncodedCredentialOfferCreated {
                offer_id: Default::default(),
                form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer,
            }]);
    }

    #[test]
    #[serial_test::serial]
    fn test_create_token_response() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
            ])
            .when(OfferCommand::CreateTokenResponse {
                offer_id: Default::default(),
                token_request: token_request(subject.clone()),
            })
            .then_expect_events(vec![OfferEvent::TokenResponseCreated {
                offer_id: Default::default(),
                token_response: token_response(subject),
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_verify_credential_response() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: Default::default(),
                    token_response: token_response(subject.clone()),
                },
            ])
            .when(OfferCommand::VerifyCredentialRequest {
                offer_id: Default::default(),
                credential_issuer_metadata: CREDENTIAL_ISSUER_METADATA.clone(),
                authorization_server_metadata: AUTHORIZATION_SERVER_METADATA.clone(),
                credential_request: credential_request(subject.clone()).await,
            })
            .then_expect_events(vec![OfferEvent::CredentialRequestVerified {
                offer_id: Default::default(),
                subject_id: SUBJECT_KEY_DID.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_credential_response() {
        *PRE_AUTHORIZED_CODES.lock().unwrap() = vec![generate_random_string()].into();
        *ACCESS_TOKENS.lock().unwrap() = vec![generate_random_string()].into();
        *C_NONCES.lock().unwrap() = vec![generate_random_string()].into();

        let subject = test_subject();
        OfferTestFramework::with(test_issuance_services())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    pre_authorized_code: subject.pre_authorized_code.clone(),
                    access_token: subject.access_token.clone(),
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer: subject.form_url_encoded_credential_offer.clone(),
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: Default::default(),
                    token_response: token_response(subject.clone()),
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: Default::default(),
                    subject_id: SUBJECT_KEY_DID.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                offer_id: Default::default(),
                signed_credentials: vec![json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT)],
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                offer_id: Default::default(),
                credential_response: credential_response(subject),
            }]);
    }

    #[derive(Clone)]
    struct TestSubject {
        subject: Arc<dyn oid4vc_core::Subject>,
        credential: String,
        access_token: String,
        pre_authorized_code: String,
        form_url_encoded_credential_offer: String,
        c_nonce: String,
    }

    lazy_static! {
        pub static ref PRE_AUTHORIZED_CODES: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref ACCESS_TOKENS: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref C_NONCES: Mutex<VecDeque<String>> = Mutex::new(vec![].into());
        pub static ref SUBJECT_KEY_DID: Arc<dyn oid4vc_core::Subject> = test_issuance_services().issuer.clone();
    }

    fn test_subject() -> TestSubject {
        let pre_authorized_code = PRE_AUTHORIZED_CODES.lock().unwrap()[0].clone();

        TestSubject {
            subject: SUBJECT_KEY_DID.clone(),
            credential: OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string(),
            pre_authorized_code: pre_authorized_code.clone(),
            access_token: ACCESS_TOKENS.lock().unwrap()[0].clone(),
            form_url_encoded_credential_offer: format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fexample.com%2F%22%2C%22credential_configuration_ids%22%3A%5B%220%22%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D"),
            c_nonce: C_NONCES.lock().unwrap()[0].clone(),
        }
    }

    fn token_request(subject: TestSubject) -> TokenRequest {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code: subject.pre_authorized_code,
            tx_code: None,
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

    async fn credential_request(subject: TestSubject) -> CredentialRequest {
        CredentialRequest {
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
            proof: Some(
                KeyProofType::builder()
                    .proof_type(ProofType::Jwt)
                    .algorithm(Algorithm::EdDSA)
                    .signer(subject.subject.clone())
                    .iss(subject.subject.identifier("did:key", Algorithm::EdDSA).await.unwrap())
                    .aud(CREDENTIAL_ISSUER_METADATA.credential_issuer.clone())
                    .iat(1571324800)
                    .nonce(subject.c_nonce.clone())
                    .subject_syntax_type("did:key")
                    .build()
                    .await
                    .unwrap(),
            ),
        }
    }

    fn credential_response(subject: TestSubject) -> CredentialResponse {
        CredentialResponse {
            credential: CredentialResponseType::Immediate {
                credential: json!(subject.credential.clone()),
                notification_id: None,
            },
            c_nonce: None,
            c_nonce_expires_in: None,
        }
    }
}
