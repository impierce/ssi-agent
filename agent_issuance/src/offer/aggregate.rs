use std::sync::Arc;

use async_trait::async_trait;
use cqrs_es::Aggregate;
use did_key::{from_existing_key, Ed25519KeyPair};
use jsonwebtoken::{Algorithm, Header};
use oid4vc_core::{jwt, Decoder, Subjects};
use oid4vc_manager::methods::key_method::KeySubject;
use oid4vci::credential_format_profiles::w3c_verifiable_credentials::jwt_vc_json::JwtVcJson;
use oid4vci::credential_format_profiles::{self, CredentialFormats};
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferQuery, CredentialsObject, Grants, PreAuthorizedCode};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseType};
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use oid4vci::VerifiableCredentialJwt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::credential::aggregate::Credential;
use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError::{self, *};
use crate::offer::event::OfferEvent;
use crate::offer::services::OfferServices;
use crate::server_config::aggregate::ServerConfig;
use crate::server_config::error::ServerConfigError::{
    MissingAuthorizationServerMetadataError, MissingCredentialIssuerMetadataError,
};

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
pub struct Offer {
    pub id: uuid::Uuid,
    // value: CredentialOfferQuery,
    // pub form_urlencoded: String,
    pub credentials: Vec<Credential>,
    pub server_config: ServerConfig,
    pub token_response: Option<TokenResponse>,
    pub credential_response: Option<CredentialResponse>,
}

#[async_trait]
impl Aggregate for Offer {
    type Command = OfferCommand;
    type Event = OfferEvent;
    type Error = OfferError;
    type Services = OfferServices;

    fn aggregate_type() -> String {
        "offer".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            // OfferCommand::CreateCredentialOffer {
            //     subject_id,
            //     pre_authorized_code,
            // } => {
            //     let mut events = vec![];

            //     let subject = self
            //         .credentials
            //         .iter()
            //         .find(|credential| credential.subject.pre_authorized_code == subject_id)
            //         .ok_or(MissingIssuanceSubjectError(subject_id.clone()))?;

            //     if let Some(pre_authorized_code) = pre_authorized_code.as_ref() {
            //         events.push(OfferEvent::PreAuthorizedCodeUpdated {
            //             subject_id: subject_id.clone(),
            //             pre_authorized_code: pre_authorized_code.clone(),
            //         });
            //     }

            //     let credential_issuer_metadata = self
            //         .oid4vci_data
            //         .credential_issuer_metadata
            //         .as_ref()
            //         .ok_or(MissingCredentialIssuerMetadataError)?;
            //     let credentials_supported = credential_issuer_metadata.credentials_supported.clone();
            //     let credential_offer = CredentialOfferQuery::CredentialOffer(OID4VCICredentialOffer {
            //         credential_issuer: credential_issuer_metadata.credential_issuer.clone(),
            //         credentials: credentials_supported
            //             .iter()
            //             .map(|cso| CredentialsObject::ByValue(cso.credential_format.clone()))
            //             .collect(),
            //         grants: Some(Grants {
            //             authorization_code: None,
            //             pre_authorized_code: Some(PreAuthorizedCode {
            //                 pre_authorized_code: pre_authorized_code
            //                     .unwrap_or_else(|| subject.pre_authorized_code.clone()),
            //                 ..Default::default()
            //             }),
            //         }),
            //     });

            //     events.push(OfferEvent::CredentialOfferCreated {
            //         subject_id: subject_id.clone(),
            //         credential_offer: CredentialOffer {
            //             value: credential_offer.clone(),
            //             form_urlencoded: credential_offer.to_string(),
            //         },
            //     });

            //     Ok(events)
            // }
            // OfferCommand::CreateTokenResponse { token_request } => match token_request {
            //     TokenRequest::PreAuthorizedCode {
            //         pre_authorized_code, ..
            //     } => {
            //         let subject_id = self
            //             .subjects
            //             .iter()
            //             .find(|subject| subject.pre_authorized_code == pre_authorized_code)
            //             .map(|subject| subject.id.clone())
            //             .ok_or(InvalidPreAuthorizedCodeError)?;

            //         #[cfg(test)]
            //         let (access_token, c_nonce) = {
            //             let access_token = tests::ACCESS_TOKENS.lock().unwrap().pop_front().unwrap();
            //             let c_nonce = tests::C_NONCES.lock().unwrap().pop_front().unwrap();
            //             (access_token, c_nonce)
            //         };
            //         #[cfg(not(test))]
            //         let (access_token, c_nonce) = { (generate_random_string(), generate_random_string()) };

            //         if self.subjects.iter().any(|subject| subject.id == subject_id) {
            //             Ok(vec![OfferEvent::TokenResponseCreated {
            //                 subject_id: subject_id.clone(),
            //                 token_response: TokenResponse {
            //                     access_token,
            //                     token_type: "bearer".to_string(),
            //                     expires_in: None,
            //                     refresh_token: None,
            //                     scope: None,
            //                     c_nonce: Some(c_nonce),
            //                     c_nonce_expires_in: None,
            //                 },
            //             }])
            //         } else {
            //             Err(InvalidPreAuthorizedCodeError)
            //         }
            //     }
            //     _ => Err(InvalidPreAuthorizedCodeError),
            // },
            // OfferCommand::CreateCredentialResponse {
            //     access_token,
            //     credential_request,
            // } => {
            //     use oid4vc_core::Subject;

            //     let subject_id = self
            //         .subjects
            //         .iter()
            //         .find(|subject| {
            //             subject
            //                 .token_response
            //                 .as_ref()
            //                 .map_or(false, |res| res.access_token == access_token)
            //         })
            //         .map(|subject| subject.id.clone())
            //         .ok_or(InvalidAccessTokenError)?;

            //     // TODO: utilize `agent_kms`.
            //     let issuer = Arc::new(KeySubject::from_keypair(
            //         from_existing_key::<Ed25519KeyPair>(b"", Some(UNSAFE_ISSUER_KEY.as_bytes())),
            //         None,
            //     ));
            //     let issuer_did = issuer.identifier().unwrap();

            //     let credential_issuer = CredentialIssuer {
            //         subject: issuer.clone(),
            //         metadata: self
            //             .oid4vci_data
            //             .credential_issuer_metadata
            //             .as_ref()
            //             .ok_or(MissingCredentialIssuerMetadataError)?
            //             .clone(),
            //         authorization_server_metadata: self
            //             .oid4vci_data
            //             .authorization_server_metadata
            //             .as_ref()
            //             .ok_or(MissingAuthorizationServerMetadataError)?
            //             .clone(),
            //     };

            //     let proof = credential_issuer
            //         .validate_proof(
            //             credential_request.proof.ok_or(MissingProofError)?,
            //             Decoder::from(&Subjects::try_from([issuer.clone() as Arc<dyn Subject>]).unwrap()),
            //         )
            //         .await
            //         .map_err(|_| InvalidProofError)?;

            //     let subject_did = proof
            //         .rfc7519_claims
            //         .iss()
            //         .as_ref()
            //         .ok_or(MissingProofIssuerError)?
            //         .clone();

            //     let mut credential = self
            //         .subjects
            //         .iter()
            //         .find(|subject| subject.id == subject_id)
            //         .ok_or(MissingIssuanceSubjectError(subject_id.clone()))?
            //         .credentials
            //         .as_ref()
            //         .ok_or(MissingCredentialError)?
            //         .unsigned_credential
            //         .clone();

            //     credential["issuer"] = json!(issuer_did);
            //     credential["credentialSubject"]["id"] = json!(subject_did);
            //     let credential_response = CredentialResponse {
            //         credential: CredentialResponseType::Immediate(CredentialFormats::JwtVcJson(
            //             credential_format_profiles::Credential {
            //                 format: JwtVcJson,
            //                 credential: json!(jwt::encode(
            //                     issuer.clone(),
            //                     Header::new(Algorithm::EdDSA),
            //                     VerifiableCredentialJwt::builder()
            //                         .sub(subject_did)
            //                         .iss(issuer_did)
            //                         .iat(0)
            //                         .exp(9999999999i64)
            //                         .verifiable_credential(credential)
            //                         .build()
            //                         .ok(),
            //                 )
            //                 .ok()),
            //             },
            //         )),
            //         c_nonce: None,
            //         c_nonce_expires_in: None,
            //     };

            //     Ok(vec![OfferEvent::CredentialResponseCreated {
            //         subject_id,
            //         credential_response,
            //     }])
            // }
            OfferCommand::CreateCredentialOffer {
                subject_id,
                pre_authorized_code,
            } => Ok(vec![]),
            OfferCommand::CreateTokenResponse { token_request } => Ok(vec![]),
            OfferCommand::CreateCredentialResponse {
                access_token,
                credential_request,
            } => Ok(vec![]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use OfferEvent::*;

        match event {
            PreAuthorizedCodeUpdated {
                // subject_id,
                pre_authorized_code,
            } => {
                if let Some(credential) = self
                    .credentials
                    .iter_mut()
                    .find(|p| p.subject.pre_authorized_code == pre_authorized_code)
                {
                    credential.subject.pre_authorized_code = pre_authorized_code;
                }
                // if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                //     subject.pre_authorized_code = pre_authorized_code;
                // }
            }
            TokenResponseCreated {
                // subject_id,
                token_response,
            } => {
                // if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                //     subject.token_response.replace(token_response);
                // }
            }
            CredentialResponseCreated {
                // subject_id,
                credential_response,
            } => {
                // if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                //     subject.credential_response.replace(credential_response);
                // }
            }
            CredentialOfferCreated {
                // subject_id,
                credential_offer,
            } => {
                // if let Some(subject) = self.subjects.iter_mut().find(|subject| subject.id == subject_id) {
                //     subject.credential_offer.replace(credential_offer);
                // }
            }
        }
    }
}
