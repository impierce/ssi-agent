use agent_shared::config::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::Validator;
use oid4vci::credential_issuer::CredentialIssuer;
use oid4vci::credential_offer::{CredentialOffer, CredentialOfferParameters, Grants, PreAuthorizedCode};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseObject, CredentialResponseType};
use oid4vci::token_request::TokenRequest;
use oid4vci::token_response::TokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info};

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
    #[serde(rename = "id")]
    pub offer_id: String,
    pub credential_offer_uri: Option<CredentialOffer>,
    pub credential_offer: Option<CredentialOffer>,
    pub subject_id: Option<String>,
    pub credential_ids: Vec<String>,
    pub form_url_encoded_credential_offer: Option<String>,
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
                credential_configuration_ids,
            } => {
                let credential_issuer = config().public_url.clone();

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

                let credential_offer = CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
                    credential_issuer: credential_issuer.clone(),
                    credential_configuration_ids,
                    grants: Some(Grants {
                        authorization_code: None,
                        pre_authorized_code: Some(PreAuthorizedCode {
                            pre_authorized_code: pre_authorized_code.clone(),
                            ..Default::default()
                        }),
                    }),
                }));

                let mut credential_offer_uri = config().credential_offer_uri.clone();
                if let Ok(mut path_segments) = credential_offer_uri.path_segments_mut() {
                    path_segments.pop_if_empty();
                    path_segments.push(&offer_id);
                }

                let credential_offer_uri = CredentialOffer::CredentialOfferUri(credential_offer_uri);

                let form_url_encoded_credential_offer = if config().credential_offer_by_value_enabled {
                    credential_offer.to_string()
                } else {
                    credential_offer_uri.to_string()
                };

                Ok(vec![
                    CredentialOfferCreated {
                        offer_id: offer_id.clone(),
                        credential_offer_uri,
                        credential_offer,
                        pre_authorized_code,
                        access_token,
                        status: Status::Created,
                    },
                    FormUrlEncodedCredentialOfferCreated {
                        offer_id,
                        form_url_encoded_credential_offer,
                        status: Status::Pending,
                    },
                ])
            }
            AddCredentials {
                offer_id,
                credential_ids,
                credential_configuration_ids,
            } => {
                let mut credential_offer = self
                    .credential_offer
                    .clone()
                    .ok_or_else(|| MissingCredentialOfferError)?;

                if let CredentialOffer::CredentialOffer(credential_offer) = &mut credential_offer {
                    // Deduplicate credential_configuration_ids to ensure uniqueness
                    let credential_configuration_id_set: HashSet<String> = credential_offer
                        .credential_configuration_ids
                        .iter()
                        .cloned()
                        .chain(credential_configuration_ids)
                        .collect();

                    credential_offer.credential_configuration_ids =
                        credential_configuration_id_set.into_iter().collect();
                } else {
                    unreachable!();
                }

                let mut events = vec![CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: credential_ids.clone(),
                    credential_offer: credential_offer.clone(),
                }];

                // If Credential Offer by Value is enabled, the form URL encoded Credential Offer needs to be updated.
                if config().credential_offer_by_value_enabled {
                    let form_url_encoded_credential_offer = credential_offer.to_string();
                    events.push(FormUrlEncodedCredentialOfferCreated {
                        offer_id: offer_id.clone(),
                        form_url_encoded_credential_offer,
                        status: Status::Pending,
                    })
                }

                Ok(events)
            }
            SendCredentialOffer { offer_id, target_url } => {
                let client = reqwest::Client::new();
                let target = self
                    .form_url_encoded_credential_offer
                    .as_ref()
                    .ok_or_else(|| MissingCredentialOfferError)?
                    .replace("openid-credential-offer://", target_url.as_str());

                info!("Sending credential offer to: {}", target);

                client
                    .get(target)
                    .send()
                    .await
                    .and_then(|response| response.error_for_status())
                    .map_err(SendCredentialOfferError)?;

                Ok(vec![CredentialOfferSent {
                    offer_id,
                    target_url,
                    status: Status::Pending,
                }])
            }
            CreateTokenResponse {
                offer_id,
                token_request,
            } => match token_request {
                TokenRequest::PreAuthorizedCode { .. } => Ok(vec![TokenResponseCreated {
                    offer_id,
                    token_response: TokenResponse {
                        access_token: self.access_token.clone(),
                        token_type: "bearer".to_string(),
                        expires_in: None,
                        refresh_token: None,
                        scope: None,
                    },
                }]),
                _ => Err(UnsupportedTokenRequestGrantTypeError),
            },
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
                let (signed_credential, notification_id) = signed_credentials.pop().ok_or(MissingCredentialError)?;
                let credential_response = CredentialResponse {
                    credential: CredentialResponseType::Immediate {
                        credentials: vec![CredentialResponseObject {
                            // TODO: Apply strong typing to signed credentials.
                            credential: signed_credential.as_str().unwrap_or_default().to_string(),
                        }],
                        notification_id,
                    },
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

        debug!("Applying event: {:?}", event);

        match event {
            CredentialOfferCreated {
                offer_id,
                pre_authorized_code,
                access_token,
                credential_offer,
                credential_offer_uri,
                status,
            } => {
                self.offer_id = offer_id;
                self.pre_authorized_code = pre_authorized_code;
                self.access_token = access_token;
                self.credential_offer.replace(credential_offer);
                self.credential_offer_uri.replace(credential_offer_uri);
                self.status = status;
            }
            CredentialsAdded {
                offer_id,
                credential_ids,
                credential_offer,
            } => {
                self.offer_id = offer_id;
                self.credential_ids = credential_ids;
                self.credential_offer.replace(credential_offer);
            }
            FormUrlEncodedCredentialOfferCreated {
                offer_id,
                form_url_encoded_credential_offer,
                status,
            } => {
                self.offer_id = offer_id;
                self.form_url_encoded_credential_offer
                    .replace(form_url_encoded_credential_offer);
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
#[allow(unused_imports)]
pub mod tests {
    use super::test_utils::*;
    use crate::credential::aggregate::test_utils::notification_id;
    use crate::{
        credential::aggregate::test_utils::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT, offer,
        server_config::aggregate::test_utils::*,
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
        offer_id: String,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
    ) {
        OfferTestFramework::with(Service::default())
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer {
                offer_id: offer_id.clone(),
                credential_configuration_ids: vec![],
            })
            .then_expect_events(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    access_token,
                    status: Status::Created,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    #[allow(clippy::too_many_arguments)]
    async fn test_add_credential(
        offer_id: String,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        credential_configuration_id: String,
        #[future(awt)] credential_offer_with_credential_configuration_ids: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer_with_credential_configuration_ids: String,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![OfferEvent::CredentialOfferCreated {
                offer_id: offer_id.clone(),
                credential_offer_uri,
                credential_offer,
                pre_authorized_code,
                access_token,
                status: Status::Created,
            }])
            .when(OfferCommand::AddCredentials {
                offer_id: offer_id.clone(),
                credential_ids: vec!["credential-id".to_string()],
                credential_configuration_ids: vec![credential_configuration_id],
            })
            .then_expect_events(vec![
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer_with_credential_configuration_ids,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id,
                    form_url_encoded_credential_offer:
                        form_url_encoded_credential_offer_with_credential_configuration_ids,
                    status: Status::Pending,
                },
            ]);
    }

    #[rstest]
    #[serial_test::serial]
    #[allow(clippy::too_many_arguments)]
    async fn test_create_token_response(
        offer_id: String,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] credential_offer_with_credential_configuration_ids: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer_with_credential_configuration_ids: String,
        #[future(awt)] token_request: TokenRequest,
        #[future(awt)] token_response: TokenResponse,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    access_token,
                    status: Status::Created,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer_with_credential_configuration_ids,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer:
                        form_url_encoded_credential_offer_with_credential_configuration_ids,
                    status: Status::Pending,
                },
            ])
            .when(OfferCommand::CreateTokenResponse {
                offer_id: offer_id.clone(),
                token_request,
            })
            .then_expect_events(vec![OfferEvent::TokenResponseCreated {
                offer_id: offer_id.clone(),
                token_response,
            }]);
    }

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
    async fn test_verify_credential_response(
        offer_id: String,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] credential_offer_with_credential_configuration_ids: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer_with_credential_configuration_ids: String,
        #[future(awt)] token_response: TokenResponse,
        #[future(awt)] credential_request: CredentialRequest,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    access_token,
                    status: Status::Created,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer_with_credential_configuration_ids,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer:
                        form_url_encoded_credential_offer_with_credential_configuration_ids,
                    status: Status::Pending,
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: offer_id.clone(),
                    token_response,
                },
            ])
            .when(OfferCommand::VerifyCredentialRequest {
                offer_id: offer_id.clone(),
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_request,
            })
            .then_expect_events(vec![OfferEvent::CredentialRequestVerified {
                offer_id: offer_id.clone(),
                subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
            }]);
    }

    #[rstest]
    #[allow(clippy::too_many_arguments)]
    #[serial_test::serial]
    async fn test_create_credential_response(
        offer_id: String,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] credential_offer_with_credential_configuration_ids: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer_with_credential_configuration_ids: String,
        #[future(awt)] token_response: TokenResponse,
        credential_response: CredentialResponse,
        notification_id: String,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    access_token,
                    status: Status::Created,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer_with_credential_configuration_ids,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer:
                        form_url_encoded_credential_offer_with_credential_configuration_ids,
                    status: Status::Pending,
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: offer_id.clone(),
                    token_response,
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: offer_id.clone(),
                    subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                offer_id: offer_id.clone(),
                signed_credentials: vec![(
                    json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT),
                    Some(notification_id.clone()),
                )],
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                offer_id: offer_id.clone(),
                credential_response,
                status: Status::Issued,
            }]);
    }

    #[rstest]
    #[allow(clippy::too_many_arguments)]
    #[serial_test::serial]
    async fn test_just_in_time_credential_flow(
        offer_id: String,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] access_token: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] credential_offer_with_credential_configuration_ids: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer_with_credential_configuration_ids: String,
        #[future(awt)] token_response: TokenResponse,
        credential_response: CredentialResponse,
        notification_id: String,
    ) {
        OfferTestFramework::with(Service::default())
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    access_token,
                    status: Status::Created,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer:
                        form_url_encoded_credential_offer_with_credential_configuration_ids,
                    status: Status::Pending,
                },
                OfferEvent::TokenResponseCreated {
                    offer_id: offer_id.clone(),
                    token_response,
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: offer_id.clone(),
                    subject_id: holder.identifier("did:key", Algorithm::EdDSA).await.unwrap(),
                },
                // Credentials are only added after the credential request is verified (JIT)
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer_with_credential_configuration_ids,
                },
            ])
            .when(OfferCommand::CreateCredentialResponse {
                offer_id: offer_id.clone(),
                signed_credentials: vec![(
                    json!(OPENBADGE_VERIFIABLE_CREDENTIAL_JWT),
                    Some(notification_id.clone()),
                )],
            })
            .then_expect_events(vec![OfferEvent::CredentialResponseCreated {
                offer_id: offer_id.clone(),
                credential_response,
                status: Status::Issued,
            }]);
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    pub use super::*;
    use crate::credential::aggregate::test_utils::notification_id;
    use crate::{
        credential::aggregate::test_utils::OPENBADGE_VERIFIABLE_CREDENTIAL_JWT, server_config::aggregate::test_utils::*,
    };
    use agent_shared::generate_random_string;
    use jsonwebtoken::Algorithm;
    use oid4vc_core::Subject;
    use oid4vci::credential_issuer::credential_configurations_supported::CredentialConfigurationsSupportedObject;
    use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId;
    use oid4vci::proof::ProofType;
    use oid4vci::Proof;
    use oid4vci::{
        credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata, credential_request::CredentialRequest,
    };
    use once_cell::sync::OnceCell;
    pub use rstest::*;
    use std::collections::HashMap;
    use url::Url;

    static PRE_AUTHORIZED_CODE: OnceCell<String> = OnceCell::new();
    static ACCESS_TOKEN: OnceCell<String> = OnceCell::new();

    #[fixture]
    pub async fn pre_authorized_code() -> String {
        PRE_AUTHORIZED_CODE.get_or_init(generate_random_string).clone()
    }

    #[fixture]
    pub async fn access_token() -> String {
        ACCESS_TOKEN.get_or_init(generate_random_string).clone()
    }

    #[fixture]
    pub async fn holder() -> Arc<dyn oid4vc_core::Subject> {
        Arc::new(agent_secret_manager::subject::Subject::default())
    }

    #[fixture]
    pub async fn credential_offer(
        #[future(awt)] pre_authorized_code: String,
        static_issuer_url: Url,
    ) -> CredentialOffer {
        CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
            credential_issuer: static_issuer_url,
            credential_configuration_ids: vec![],
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
    pub async fn credential_offer_with_credential_configuration_ids(
        #[future(awt)] mut credential_offer: CredentialOffer,
        credential_configurations_supported: HashMap<String, CredentialConfigurationsSupportedObject>,
    ) -> CredentialOffer {
        if let CredentialOffer::CredentialOffer(credential_offer) = &mut credential_offer {
            credential_offer.credential_configuration_ids =
                credential_configurations_supported.keys().cloned().collect();
        } else {
            unreachable!();
        }

        credential_offer
    }

    #[fixture]
    pub async fn credential_offer_uri(
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        offer_id: String,
    ) -> CredentialOffer {
        CredentialOffer::CredentialOfferUri(
            credential_issuer_metadata
                .credential_issuer
                .join("openid4vci/")
                .and_then(|url| url.join("credential-offer/").and_then(|url| url.join(&offer_id)))
                .expect("Failed to parse URL in test"),
        )
    }

    #[fixture]
    pub async fn form_url_encoded_credential_offer(#[future(awt)] pre_authorized_code: String) -> String {
        format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fmy-domain.example.org%2F%22%2C%22credential_configuration_ids%22%3A%5B%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D")
    }

    #[fixture]
    pub async fn form_url_encoded_credential_offer_with_credential_configuration_ids(
        #[future(awt)] pre_authorized_code: String,
    ) -> String {
        format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fmy-domain.example.org%2F%22%2C%22credential_configuration_ids%22%3A%5B%22001%22%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D")
    }

    #[fixture]
    pub fn offer_id() -> String {
        "offer_id".to_string()
    }
    #[fixture]
    pub async fn token_request(#[future(awt)] pre_authorized_code: String) -> TokenRequest {
        TokenRequest::PreAuthorizedCode {
            pre_authorized_code,
            tx_code: None,
        }
    }

    #[fixture]
    pub async fn token_response(#[future(awt)] access_token: String) -> TokenResponse {
        TokenResponse {
            access_token,
            token_type: "bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
        }
    }

    #[fixture]
    pub async fn credential_request(
        credential_configuration_id: String,
        #[future(awt)] holder: Arc<dyn Subject>,
        static_issuer_url: Url,
    ) -> CredentialRequest {
        CredentialRequest {
            credential_identifier_or_credential_configuration_id: CredentialConfigurationId(
                credential_configuration_id,
            ),
            proof: Some(
                Proof::builder()
                    .proof_type(ProofType::Jwt)
                    .algorithm(Algorithm::EdDSA)
                    .signer(holder.clone())
                    .iss(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap())
                    .aud(static_issuer_url.to_string())
                    .iat(1571324800)
                    .subject_syntax_type("did:key")
                    .build()
                    .await
                    .unwrap(),
            ),
            proofs: None,
        }
    }

    #[fixture]
    pub fn credential_response(notification_id: String) -> CredentialResponse {
        CredentialResponse {
            credential: CredentialResponseType::Immediate {
                credentials: vec![CredentialResponseObject {
                    credential: OPENBADGE_VERIFIABLE_CREDENTIAL_JWT.to_string(),
                }],
                notification_id: Some(notification_id.clone()),
            },
        }
    }
}
