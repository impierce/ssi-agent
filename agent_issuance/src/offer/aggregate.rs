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
                    .post(target_url.clone())
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

    pub mod temp {
        use std::str::FromStr as _;

        use super::*;
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        use identity_core::common::{Timestamp, Url};
        use identity_credential::{
            sd_jwt_payload::SdJwt,
            sd_jwt_v2::{KeyBindingJwt, Sha256Hasher},
            sd_jwt_vc::{
                tests::{
                    holder_signer_secret_jwk, signer_secret_jwk, validation::test_resolver, TestHolderSigner,
                    TestJwsVerifier, TestSigner,
                },
                SdJwtVc, SdJwtVcBuilder, SdJwtVcPresentationBuilder,
            },
        };
        use identity_iota::{
            core::{FromJson as _, ToJson as _},
            credential::KeyBindingJWTValidationOptions,
            verification::{
                jwk::Jwk,
                jws::{JwsVerifier, SignatureVerificationError, VerificationInput},
            },
        };
        use jsonwebtoken::{crypto::verify, DecodingKey, Validation};
        use serde_json::json;

        /// This `Verifier` uses `jsonwebtoken` under the hood to verify verification input.
        pub struct Verifier;
        impl JwsVerifier for Verifier {
            fn verify(&self, input: VerificationInput, public_key: &Jwk) -> Result<(), SignatureVerificationError> {
                let algorithm = Algorithm::from_str(&input.alg.to_string()).unwrap();

                println!("public_key: {:?}", public_key);

                // Convert the `Jwk` first into a `jsonwebtoken::jwk::Jwk` and then into a `DecodingKey`.
                let decoding_key = public_key
                    .to_json()
                    .ok()
                    .and_then(|public_key| jsonwebtoken::jwk::Jwk::from_json(&public_key).ok())
                    .and_then(|jwk| DecodingKey::from_jwk(&jwk).ok())
                    .unwrap();

                let mut validation = Validation::new(algorithm);
                validation.validate_aud = false;
                validation.required_spec_claims.clear();

                println!("validation: {:?}", validation);

                verify(
                    &URL_SAFE_NO_PAD.encode(input.decoded_signature),
                    &input.signing_input,
                    &decoding_key,
                    algorithm,
                )
                .unwrap();

                Ok(())
            }
        }

        #[rstest]
        async fn building_valid_vc_works() {
            let credential = json!({
              "name": "John Doe",
              "birthdate": "1970-01-01"
            });

            let sd_jwt = SdJwtVcBuilder::new(credential)
                .unwrap()
                .vct("https://bmi.bund.example/credential/pid/1.0".parse::<Url>().unwrap())
                .iat(Timestamp::now_utc())
                .iss("https://example.com/".parse().unwrap())
                .require_key_binding(identity_credential::sd_jwt_v2::RequiredKeyBinding::Kid(
                    "did:example:123#key-0".to_string(),
                ))
                .finish(&TestSigner, "HS256")
                .await
                .unwrap();

            println!("{}", sd_jwt);
        }

        #[rstest]
        async fn building_vc_with_disclosed_mandatory_claim_fails() {
            let credential = json!({
                "given_name": "John",
                "family_name": "Doe",
                "birthdate": "1940-01-01"
            });

            let sd_jwt = SdJwtVcBuilder::new(credential)
                .unwrap()
                .vct(
                    "https://credentials.example.com/identity_credential"
                        .parse::<Url>()
                        .unwrap(),
                )
                .iat(Timestamp::now_utc())
                .iss("https://example.com/issuer".parse().unwrap())
                .make_concealable("/given_name")
                .unwrap()
                .make_concealable("/family_name")
                .unwrap()
                .make_concealable("/birthdate")
                .unwrap()
                .finish(&TestSigner, "HS256")
                .await
                .unwrap();

            println!("{}", sd_jwt);
        }

        #[test]
        fn temop() {
            let string = "eyJjdHkiOiJjcmVkZW50aWFsLWNsYWltcy1zZXQranNvbiIsInR5cCI6InZjK3NkLWp3dCIsImFsZyI6IkVTMjU2Iiwia2lkIjoiZGlkOmp3azpleUpyZEhraU9pSkZReUlzSW1OeWRpSTZJbEF0TWpVMklpd2lhMmxrSWpvaVdYSjBiVFpmYW10bFQxWkRlbHB6UTIxNlpUZE1ZM1I2Vm1jd1ZrOXhlblJoU1VaRFNrUTVVRWRmT0NJc0luZ2lPaUoxY0d0TVpGOXBiMk5QT0VKWFZFaFRNRGhyYVUxdlVDMUxhVmhhUVVOSWNVcE5OMFJSWkROVmRGRkZJaXdpZVNJNklpMXNRelpxV0ZkWWFrRlNORmxMZERWTVVFVlNVMWhyU1ZZNFExVTJTelZWZG1sWlVVZERTRFV6Tm1NaWZRIn0.eyJpZCI6InVybjp1dWlkOmRjNGQyNDdkLWZlZGEtNDA2Ni1hMzE0LWNiNTUyYzM3OTdkYiIsImlzcyI6ImRpZDpqd2s6ZXlKcmRIa2lPaUpGUXlJc0ltTnlkaUk2SWxBdE1qVTJJaXdpYTJsa0lqb2lXWEowYlRaZmFtdGxUMVpEZWxwelEyMTZaVGRNWTNSNlZtY3dWazl4ZW5SaFNVWkRTa1E1VUVkZk9DSXNJbmdpT2lKMWNHdE1aRjlwYjJOUE9FSlhWRWhUTURocmFVMXZVQzFMYVZoYVFVTkljVXBOTjBSUlpETlZkRkZGSWl3aWVTSTZJaTFzUXpacVdGZFlha0ZTTkZsTGREVk1VRVZTVTFoclNWWTRRMVUyU3pWVmRtbFpVVWREU0RVek5tTWlmUSIsImlhdCI6MTcyODkxMDU5MCwibmJmIjoxNzI4OTEwNTkwLCJleHAiOjE3NjA0NDY1OTAsIl9zZCI6WyIxNWoxM1JKMVRMMDJ4TFhhaWV4ZEE1bVZDM1dTdkFpMVdWdjJrNHBKOGc0IiwibnV2MExsdVhDWk9KMkxrd05ZS3NrNXl3ZlRKNnlxV0c2VWtCQ2JGRFVObyIsIkNxeDA1RFFrNlRMNTJhZjN2NU12UnlhUnZoUWxHQkJaWERFVWlxN3M3RnMiLCJTTmsyVHVkRl9EQXk4LVpSbEJMZGtpRHJMc1VjNlJlLV9rQkhhdVN3dU5RIiwiclJtNmdWbUxkSExHa1luNXdyOGszXzQ3MWF4cHB2RzRPZ2NLblRTcGxrZyIsIlhKRVJYblc0eDV0WHk0UUJlbTdDczNJUXVycDJRUDhZNlVtTGwwRmZtQjgiLCJNVURlaEpOZVBwVGFVNFlIRFRlNzZMZV8tdHh0ZEhaSFdpb1l4aU90OXNRIiwiR0M5RlluSGVBX3ZSaHE3RWtXYkVFbkxIRFFOSlRpWEY1VXFNQTQtM1hxUSIsImREM1ZFMDB2QUxXNXRBenFfZ0ozbV95c1FqRU54NlRiSVZqTzN2UV9QSHMiLCJBVktUWEFZckxtNlM0VVM0NGcxRlUzVDh6OWRsdU1xMk41ZWRYRV84cEtFIiwibUxOWVU1N2hBeUcwenpzMGpCdnBMYjMzckZOUU5YUV90M1hUeWJ0Y2hyZyIsIk11QWdqOGVqVkdWajM0Z0I5dXhFaDFPWVZWNVNUaHhvbTM3UDZzeWVfNlkiLCJFaHZNOW5WS2szNG5fVkZXemNNSElzTkM4NElQNG54a3hYOE9Wc3pkS2NJIiwic1ZEWHp1dUc0TlgteU91T1ZxVkF6SVdUdmZ4M1VteHRRWkw5NVJiY3lDYyIsIlBmRzJhZHpsYkxmYkY0UFdDeEVFLW12eDhpWlVxQ2VYSDlwbHZDX3ZFZHciLCI1azY5dWRkWVd5YVJWRjRVZFdzaTJXNDRuaElTMk1zTWc5V2RRdUZyVktZIiwiOWpJTnJUYUh4X2RzVjE4aF9nRjFEZUNjbzlrZ0xZdHdfN3A4Ujk0UjRsdyIsIm1UQS1PQUZabmduWVdWdmZyNkNXTG1CbFZEaUdpUWJoMGpmTWpqMU44bHciLCJEckZlOGUtMnJhSGNzd1pyRW9pOG1ubFRMNDdaX00ybWVLbzN2TFdqMDdzIiwiUnYyNmlhWi1vLThidzVsd1JUSzNnaG9KQkowdi00ZjJZdWRWeXFVVDBPSSIsIjBWT1RrVloxaHRpWFlfTm81V2RoX1BrQTdGcFRhZUNqRHc2ZWg1TnpiQ2siLCJvYVZTUTktRzZnYjIwSmgxWnd3TFNZNXVRRFg2RjV2T1BNTTZnMVp1RG5rIiwiZFlMYmpISjJjdk9yaHlCNWRCMkI3NmFOb1NIbjlDcWotS2poamZreEYzdyIsIkZLVVdnUVlicHF5dWhiS0Zidk9aZzhacE1DeWUycUxpRHVYZUJCSkMtVDQiLCJscm96eV85dlNGOTM5enk0V1N0VF9BSVQ5dnBiV3BvaUtmeTJyYTI1a1RzIiwia1NrVlVSRzVianhQNmpYM2xvUXVYQ0N2bTNBdmZwUUVUckkyLTFaRWVBVSJdfQ.gV-KWqsU58ApynFspMcUAmUe5D1rkxAB7kZ66DHWHRSc0Ty1KukLmbdZMt_3q0AH18ePgBxiM0JrtFc8sLy85g~WyJNSFRBT0IwVkduNllBQXUyeFRqeE9nPT0iLCJmYW1pbHlfbmFtZSIsIlZpc3NlciJd~WyI5Tlh6Q2RHMWxnUGNjSHBfd24xWFd3PT0iLCJnaXZlbl9uYW1lIiwiQ2hhcmxvdHRlIl0~WyIzbjRWamlpa0JIdHZreUhfZjAxa1FBPT0iLCJiaXJ0aF9kYXRlIiwiMTk5MC0wNy0yMiJd~WyJEYlp3QjdjLUk1UEFURUdpSlNLZmF3PT0iLCJmYW1pbHlfbmFtZV9iaXJ0aCIsIlZpc3NlciJd~WyJLYVBaQnMwY280QWhXdjU2LXJsazd3PT0iLCJnaXZlbl9uYW1lX2JpcnRoIiwiQ2hhcmxvdHRlIl0~WyJ2ZklKS19tSEl1WEVRSVVFWnlhbTdBPT0iLCJiaXJ0aF9wbGFjZSIsIlV0cmVjaHQiXQ~WyJpWTJ5UnBWMVlaMDJ5cGxsaXRXNzNBPT0iLCJiaXJ0aF9jb3VudHJ5IiwiTmV0aGVybGFuZHMiXQ~WyJVcEF2UlhXcUdTV3dzWVVaUVNJRXFRPT0iLCJiaXJ0aF9zdGF0ZSIsIlV0cmVjaHQiXQ~WyJKUDhWZ0pCbmF1cjA0cW50MDRHQldBPT0iLCJiaXJ0aF9jaXR5IiwiVXRyZWNodCJd~WyJqZlFZbGV4dThSemNOeVlnaktWT2ZRPT0iLCJyZXNpZGVudF9hZGRyZXNzIiwiT3VkZWdyYWNodCA0NTYiXQ~WyJfZEtYeWE3aHVUUTZScUZNcG9STFRnPT0iLCJyZXNpZGVudF9jb3VudHJ5IiwiTmV0aGVybGFuZHMiXQ~WyJWX09WYU5PVUFGcWpBNDFtM2trM0ZBPT0iLCJyZXNpZGVudF9zdGF0ZSIsIlV0cmVjaHQiXQ~WyJrUy0yQmd1dllpYmFKTGo0R3BIVGpBPT0iLCJyZXNpZGVudF9jaXR5IiwiVXRyZWNodCJd~WyJwRDNjSWExV29FVWZyY0NiWFFweTRnPT0iLCJyZXNpZGVudF9wb3N0YWxfY29kZSIsIjM1MTEgQVMiXQ~WyI0WG00Vk11WUpITFVIaTBQcHBxR3dnPT0iLCJyZXNpZGVudF9zdHJlZXQiLCJPdWRlZ3JhY2h0Il0~WyJpWTBHWGh4M3U2Z3ZqblQtN3pvWWt3PT0iLCJyZXNpZGVudF9ob3VzZV9udW1iZXIiLCI0NTYiXQ~WyJqa21qUmxhOW91NHB1WEtvVzV5U3RnPT0iLCJnZW5kZXIiLCJGZW1hbGUiXQ~WyJ6TzRPMVdYcHNRYS00ZFMtbTU4b1p3PT0iLCJuYXRpb25hbGl0eSIsIkR1dGNoIl0~WyJVUEFMWVZxVXM3THE1bklISmJRaTZnPT0iLCJkb2N1bWVudF9udW1iZXIiLCJERUY3ODkwMTIiXQ~WyJzZUdGUUlqRUtLZkRKMEppdW9oV1lnPT0iLCJhZG1pbmlzdHJhdGl2ZV9udW1iZXIiLCIxMjAwMjEzNDEyIl0~WyJ6QnhHcnZWZmZsbTJfeTIxRldXSGpBPT0iLCJpc3N1aW5nX2NvdW50cnkiLCJOZXRoZXJsYW5kcyJd~WyItLUVCeEJNUWp1TmhTenEtUWFENkhRPT0iLCJpc3N1aW5nX2p1cmlzZGljdGlvbiIsIk5MLVVUIl0~WyJPb2F5T01laDQtaHJHMzJjRWhuZ1RnPT0iLCJhZ2Vfb3Zlcl8xOCIsdHJ1ZV0~WyJZTnZpcmRIbzE2OE5RWEFkYXZGbG1RPT0iLCJhZ2Vfb3Zlcl8yMSIsZmFsc2Vd~WyJYSmFDbUdwRWg1OGxMeGtna09PUzJBPT0iLCJhZ2VfaW5feWVhcnMiLDMxXQ~WyJxbWM0Skw1YXlpNjVzeXJrMW5iMHJnPT0iLCJhZ2VfYmlydGhfeWVhciIsMTk5MF0".to_string();

            let input = SdJwtVc::from_str(&string).unwrap();

            println!("{}", input);
        }

        #[rstest]
        async fn validation_of_valid_token_works() {
            let now = Timestamp::from_unix(0).unwrap();

            let sd_jwt_credential = SdJwtVcBuilder::new(json!({
              "name": "John Doe",
              "address": {
                "street_address": "A random street",
                "number": "3a"
              },
              "degree": []
            }))
            .unwrap()
            .header(std::iter::once(("kid".to_string(), serde_json::Value::String("key1".to_string()))).collect())
            .vct("https://example.com/education_credential".parse::<Url>().unwrap())
            .iat(now)
            .iss("https://example.com".parse().unwrap())
            .require_key_binding(identity_credential::sd_jwt_v2::RequiredKeyBinding::Kid(
                "key1".to_string(),
            ))
            // .make_concealable("/address/street_address")
            // .unwrap()
            // .make_concealable("/address")
            // .unwrap()
            .finish(&TestSigner, "HS256")
            .await
            .unwrap();

            println!("here now: {}", sd_jwt_credential);

            let kb_jwt = KeyBindingJwt::builder()
                .iat(now.to_unix())
                .aud("test-verifier")
                .nonce("test-verifier-nonce")
                .finish(&sd_jwt_credential, &Sha256Hasher::new(), "HS256", &TestHolderSigner)
                .await
                .unwrap();

            let (sd_jwt_vc, _) = SdJwtVcPresentationBuilder::new(sd_jwt_credential, &Sha256Hasher::new())
                .unwrap()
                .attach_key_binding_jwt(kb_jwt)
                .finish()
                .unwrap();

            println!("this: {}", sd_jwt_vc);

            // let sd_jwt_vc = SdJwtVc::from_str("eyJraWQiOiJrZXkxIiwidHlwIjoidmMrc2Qtand0IiwiYWxnIjoiSFMyNTYifQ.eyJuYW1lIjoiSm9obiBEb2UiLCJkZWdyZWUiOltdLCJfc2QiOlsiaF8xNWJWRUNXWDE5bjRHVUF0cHM4OFlNcE9Id1doTWpUXzUwNFY1clc3OCJdLCJpc3MiOiJodHRwczovL2V4YW1wbGUuY29tLyIsImlhdCI6MCwidmN0IjoiaHR0cHM6Ly9leGFtcGxlLmNvbS9lZHVjYXRpb25fY3JlZGVudGlhbCIsIl9zZF9hbGciOiJzaGEtMjU2IiwiY25mIjp7ImtpZCI6ImtleTEifX0.91VO5nOv1B910pIEU3T2J749yEBdKRn4UId6FYertGM~WyJwRzI4VFczdjJvRUtCME9DbUQweWFiTzRFUHg2blpJdVZjQ0ZYNmhnIiwiYWRkcmVzcyIseyJudW1iZXIiOiIzYSIsIl9zZCI6WyJoUUY0R0JWNU9VWE5qdVIzaDhFXzZSRXp1S2NzNk9MeV9URGswa19HRmt3Il19XQ~WyJ5eWNRRDFsalJCUHY4aWxHQ1ZMZ1NnbXM0X3NOdUVrRnhGa05QbVhtIiwic3RyZWV0X2FkZHJlc3MiLCJBIHJhbmRvbSBzdHJlZXQiXQ~eyJhbGciOiJIUzI1NiIsInR5cCI6ImtiK2p3dCJ9.eyJpYXQiOjAsImF1ZCI6InRlc3QtdmVyaWZpZXIiLCJub25jZSI6InRlc3QtdmVyaWZpZXItbm9uY2UiLCJzZF9oYXNoIjoiRlI2ZUprS3ZKYmRXekR1S1F1VGI1aURSdFhIaE1TT0RqYmFnUzY0eWNQVSJ9.shQCf7mZl83fjnfTjTcLGpG-vctRvuX_eBDpb6a9jh0").unwrap();

            sd_jwt_vc
                .validate_key_binding(
                    &TestJwsVerifier,
                    &holder_signer_secret_jwk(),
                    &Sha256Hasher::new(),
                    &KeyBindingJWTValidationOptions::default(),
                )
                .unwrap();

            println!("sd_jwt_vc: {}", sd_jwt_vc);

            // let resolver = test_resolver();
            // sd_jwt_vc
            //     .validate(&resolver, &TestJwsVerifier, &Sha256Hasher::new())
            //     .await
            //     .unwrap();
        }
    }

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
