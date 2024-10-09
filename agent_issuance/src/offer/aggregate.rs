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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Created,
    Pending,
    Issued,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Offer {
    pub offer_id: String,
    pub credential_offer: Option<CredentialOffer>,
    pub subject_id: Option<String>,
    pub credential_ids: Vec<String>,
    pub form_url_encoded_credential_offer: String,
    pub pre_authorized_code: String,
    pub token_response: Option<TokenResponse>,
    pub access_token: String,
    pub credential_response: Option<CredentialResponse>,
    pub status: Status,
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
            CreateCredentialOffer {
                offer_id,
                credential_issuer_metadata,
            } => {
                #[cfg(feature = "test_utils")]
                let (pre_authorized_code, access_token) = {
                    let pre_authorized_code = test_utils::pre_authorized_code().await;
                    let access_token = test_utils::access_token().await;
                    (pre_authorized_code, access_token)
                };
                #[cfg(not(feature = "test_utils"))]
                let (pre_authorized_code, access_token) = {
                    (
                        agent_shared::generate_random_string(),
                        agent_shared::generate_random_string(),
                    )
                };

                // TODO: This needs to be fixed when we implement Batch credentials.
                let credentials_supported = credential_issuer_metadata.credential_configurations_supported.clone();
                let credential_offer = CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
                    credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
                    credential_configuration_ids: credentials_supported.keys().cloned().collect(),
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: pre_authorized_code.clone(),
                            ..Default::default()
                        }),
                    }),
                }));

                Ok(vec![CredentialOfferCreated {
                    offer_id,
                    credential_offer,
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
            CreateFormUrlEncodedCredentialOffer { offer_id } => Ok(vec![FormUrlEncodedCredentialOfferCreated {
                offer_id,
                form_url_encoded_credential_offer: self
                    .credential_offer
                    .as_ref()
                    .ok_or(MissingCredentialOfferError)?
                    .to_string(),
                status: Status::Pending,
            }]),
            SendCredentialOffer { offer_id, target_url } => {
                // TODO: add to `service`?
                let client = reqwest::Client::new();

                client
                    .get(target_url.clone())
                    .json(self.credential_offer.as_ref().ok_or(MissingCredentialOfferError)?)
                    .send()
                    .await
                    .map_err(|e| SendCredentialOfferError(e.to_string()))?;

                Ok(vec![CredentialOfferSent {
                    offer_id,
                    target_url,
                    status: Status::Pending,
                }])
            }
            CreateTokenResponse {
                offer_id,
                token_request,
            } => {
                #[cfg(feature = "test_utils")]
                let c_nonce = test_utils::c_nonce().await;
                #[cfg(not(feature = "test_utils"))]
                let c_nonce = agent_shared::generate_random_string();

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
                    metadata: *credential_issuer_metadata,
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
                    status: Status::Issued,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            CredentialOfferCreated {
                offer_id,
                pre_authorized_code,
                access_token,
                credential_offer,
            } => {
                self.offer_id = offer_id;
                self.pre_authorized_code = pre_authorized_code;
                self.access_token = access_token;
                self.credential_offer.replace(credential_offer);
            }
            CredentialsAdded {
                offer_id,
                credential_ids,
            } => {
                self.offer_id = offer_id;
                self.credential_ids = credential_ids;
            }
            FormUrlEncodedCredentialOfferCreated {
                offer_id,
                form_url_encoded_credential_offer,
                status,
            } => {
                self.offer_id = offer_id;
                self.form_url_encoded_credential_offer = form_url_encoded_credential_offer;
                self.status = status;
            }
            CredentialOfferSent { .. } => {}
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
    use super::test_utils::*;
    use crate::{
        credential::aggregate::test_utils::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT, server_config::aggregate::test_utils::*,
    };
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use jsonwebtoken::Algorithm;
    use oid4vc_core::Subject;
    use oid4vci::{
        credential_issuer::{
            authorization_server_metadata::AuthorizationServerMetadata,
            credential_issuer_metadata::CredentialIssuerMetadata,
        },
        credential_request::CredentialRequest,
    };

    use serde_json::json;

    type OfferTestFramework = TestFramework<Offer>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_offer(
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        #[future(awt)] credential_offer: CredentialOffer,
    ) {
        OfferTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer {
                offer_id: Default::default(),
                credential_issuer_metadata,
            })
            .then_expect_events(vec![OfferEvent::CredentialOfferCreated {
                offer_id: Default::default(),
                credential_offer,
                pre_authorized_code,
                access_token,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_add_credential(
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferCreated {
                offer_id: Default::default(),
                credential_offer,
                pre_authorized_code,
                access_token,
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

    #[rstest]
    #[serial_test::serial]
    async fn test_create_credential_offer(
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    credential_offer,
                    pre_authorized_code,
                    access_token,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
            ])
            .when(OfferCommand::CreateFormUrlEncodedCredentialOffer {
                offer_id: Default::default(),
            })
            .then_expect_events(vec![OfferEvent::FormUrlEncodedCredentialOfferCreated {
                offer_id: Default::default(),
                form_url_encoded_credential_offer,
                status: Status::Pending,
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_token_response(
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        #[future(awt)] token_request: TokenRequest,
        #[future(awt)] token_response: TokenResponse,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    credential_offer,
                    pre_authorized_code,
                    access_token,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
            ])
            .when(OfferCommand::CreateTokenResponse {
                offer_id: Default::default(),
                token_request,
            })
            .then_expect_events(vec![OfferEvent::TokenResponseCreated {
                offer_id: Default::default(),
                token_response,
            }]);
    }

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
    async fn test_verify_credential_response(
        holder: &Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        #[future(awt)] token_response: TokenResponse,
        #[future(awt)] credential_request: CredentialRequest,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    credential_offer,
                    pre_authorized_code,
                    access_token,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: Default::default(),
                    token_response,
                },
            ])
            .when(OfferCommand::VerifyCredentialRequest {
                offer_id: Default::default(),
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_request,
            })
            .then_expect_events(vec![OfferEvent::CredentialRequestVerified {
                offer_id: Default::default(),
                subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
            }]);
    }

    #[rstest]
    #[serial_test::serial]
    async fn test_create_credential_response(
        holder: &Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        #[future(awt)] token_response: TokenResponse,
        credential_response: CredentialResponse,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: Default::default(),
                    credential_offer,
                    pre_authorized_code,
                    access_token,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: Default::default(),
                    credential_ids: vec!["credential-id".to_string()],
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: Default::default(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: Default::default(),
                    token_response,
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: Default::default(),
                    subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                offer_id: Default::default(),
                signed_credentials: vec![json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT)],
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                offer_id: Default::default(),
                credential_response,
                status: Status::Issued,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    pub use super::*;
    use crate::{
        credential::aggregate::test_utils::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT, server_config::aggregate::test_utils::*,
    };
    use agent_secret_manager::service::Service;
    use agent_shared::generate_random_string;
    use jsonwebtoken::Algorithm;
    use lazy_static::lazy_static;
    use oid4vc_core::Subject;
    use oid4vci::{
        credential_format_profiles::{
            w3c_verifiable_credentials::jwt_vc_json::CredentialDefinition, CredentialFormats, Parameters,
        },
        credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata,
        credential_request::CredentialRequest,
        KeyProofType, ProofType,
    };
    use once_cell::sync::OnceCell;
    pub use rstest::*;
    use serde_json::json;
    use url::Url;

    lazy_static! {
        pub static ref SUBJECT_KEY_DID: Arc<dyn oid4vc_core::Subject> = IssuanceServices::default().issuer.clone();
    }

    static PRE_AUTHORIZED_CODE: OnceCell<String> = OnceCell::new();
    static ACCESS_TOKEN: OnceCell<String> = OnceCell::new();
    static C_NONCE: OnceCell<String> = OnceCell::new();

    #[fixture]
    pub async fn pre_authorized_code() -> String {
        PRE_AUTHORIZED_CODE.get_or_init(generate_random_string).clone()
    }

    #[fixture]
    pub async fn access_token() -> String {
        ACCESS_TOKEN.get_or_init(generate_random_string).clone()
    }

    #[fixture]
    pub async fn c_nonce() -> String {
        C_NONCE.get_or_init(generate_random_string).clone()
    }

    pub struct TestAttributes {
        pub pre_authorized_code: String,
        pub access_token: String,
        pub c_nonce: String,
    }

    #[fixture]
    pub async fn attributes(
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] c_nonce: String,
    ) -> TestAttributes {
        TestAttributes {
            pre_authorized_code,
            access_token,
            c_nonce,
        }
    }

    #[fixture]
    #[once]
    pub fn holder() -> Arc<dyn oid4vc_core::Subject> {
        SUBJECT_KEY_DID.clone()
    }

    #[fixture]
    pub async fn credential_offer(
        #[future(awt)] pre_authorized_code: String,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
    ) -> CredentialOffer {
        CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
            credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
            credential_configuration_ids: credential_issuer_metadata
                .credential_configurations_supported
                .keys()
                .cloned()
                .collect(),
            grants: Some(Grants {
                authorization_code: None,
                pre_authorized_code: Some(PreAuthorizedCode {
                    pre_authorized_code,
                    ..Default::default()
                }),
            }),
        }))
    }

    #[fixture]
    pub async fn form_url_encoded_credential_offer(#[future(awt)] pre_authorized_code: String) -> String {
        format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fexample.com%2F%22%2C%22credential_configuration_ids%22%3A%5B%22badge%22%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D")
    }

    #[fixture]
    pub async fn token_request(#[future(awt)] pre_authorized_code: String) -> TokenRequest {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code,
            tx_code: None,
        }
    }

    #[fixture]
    pub async fn token_response(#[future(awt)] access_token: String, #[future(awt)] c_nonce: String) -> TokenResponse {
        TokenResponse {
            access_token,
            token_type: "bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
            c_nonce: Some(c_nonce),
            c_nonce_expires_in: None,
        }
    }

    #[fixture]
    pub async fn credential_request(
        #[future(awt)] c_nonce: String,
        holder: &Arc<dyn Subject>,
        static_issuer_url: &Url,
    ) -> CredentialRequest {
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
                    .signer(holder.clone())
                    .iss(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap())
                    .aud(static_issuer_url.to_string())
                    .iat(1571324800)
                    .nonce(c_nonce)
                    .subject_syntax_type("did:key")
                    .build()
                    .await
                    .unwrap(),
            ),
        }
    }

    #[fixture]
    pub fn credential_response() -> CredentialResponse {
        CredentialResponse {
            credential: CredentialResponseType::Immediate {
                credential: json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string()),
                notification_id: None,
            },
            c_nonce: None,
            c_nonce_expires_in: None,
        }
    }
}
