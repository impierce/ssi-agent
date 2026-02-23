use agent_shared::config::config;
use async_trait::async_trait;
use cqrs_es::Aggregate;
use oid4vc_core::Validator;
use oid4vci::credential_issuer::CredentialIssuer;
use oid4vci::credential_offer::{
    AuthorizationCode, CredentialOffer, CredentialOfferParameters, GrantType, Grants, PreAuthorizedCode,
};
use oid4vci::credential_response::{CredentialResponse, CredentialResponseObject, CredentialResponseType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info};
use url::Url;

use crate::offer::command::OfferCommand;
use crate::offer::error::OfferError::{self, *};
use crate::offer::event::OfferEvent;
use crate::services::IssuanceServices;
use crate::utils::generate_tx_code::generate_tx_code;
use oid4vci::credential_offer::CredentialConfigurationIds;
use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId;

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
    pub grant_types: Vec<GrantType>,
    pub credential_offer_uri: Option<CredentialOffer>,
    pub credential_offer: Option<CredentialOffer>,
    pub subject_id: Option<String>,
    pub credential_ids: Vec<String>,
    pub form_url_encoded_credential_offer: Option<String>,
    pub pre_authorized_code: String,
    pub credential_response: Option<CredentialResponse>,
    pub status: Status,
    pub tx_code: Option<String>,
    pub delivery_options: Option<DeliveryOptions>,
    pub offer_link: Option<Url>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DeliveryOptions {
    pub recipient_email: Option<String>,
}

// Delivery methods for sending the credential offer. Not to be confused
// with the DeliveryOptions struct, which is used when creating the offer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DeliveryMethod {
    TargetUrl {
        #[serde(rename = "targetUrl")]
        target_url: Url,
    },
    Email {
        #[serde(rename = "recipientEmail")]
        recipient_email: String,
    },
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
                grant_types,
                credential_configuration_ids,
                tx_code_constraints,
                delivery_options,
            } => {
                let credential_issuer = config().public_url.clone();

                #[cfg(feature = "test_utils")]
                let pre_authorized_code = test_utils::pre_authorized_code().await;
                #[cfg(not(feature = "test_utils"))]
                let pre_authorized_code = agent_shared::generate_random_string();

                let grants = Grants {
                    authorization_code: grant_types.contains(&GrantType::AuthorizationCode).then(|| {
                        AuthorizationCode {
                            issuer_state: Some(offer_id.clone()),
                            authorization_server: None,
                        }
                    }),
                    pre_authorized_code: grant_types.contains(&GrantType::PreAuthorizedCode).then(|| {
                        PreAuthorizedCode {
                            pre_authorized_code: pre_authorized_code.clone(),
                            tx_code: tx_code_constraints.clone(),
                            ..Default::default()
                        }
                    }),
                };

                // If TxCode constraints are provided, generate a transaction code.
                let tx_code = tx_code_constraints.as_ref().map(generate_tx_code);

                let credential_offer = CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
                    credential_issuer: credential_issuer.clone(),
                    credential_configuration_ids: CredentialConfigurationIds::try_new(credential_configuration_ids)
                        .map_err(|_| OfferError::MissingCredentialConfigurationIdsError)?,
                    grants: Some(grants),
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

                let mut events = vec![
                    CredentialOfferCreated {
                        offer_id: offer_id.clone(),
                        grant_types,
                        credential_offer_uri,
                        credential_offer: credential_offer.clone(),
                        pre_authorized_code,
                        status: Status::Created,
                        tx_code: tx_code.clone(),
                        delivery_options: delivery_options.clone(),
                    },
                    FormUrlEncodedCredentialOfferCreated {
                        offer_id: offer_id.clone(),
                        form_url_encoded_credential_offer: form_url_encoded_credential_offer.clone(),
                        status: Status::Pending,
                    },
                ];

                // Emit TxCodeGenerated event if a transaction code was generated
                if let Some(tx_code_value) = tx_code {
                    events.push(TxCodeGenerated {
                        offer_id: offer_id.clone(),
                        tx_code: tx_code_value,
                        delivery_options: delivery_options.clone(),
                    });
                }

                Ok(events)
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
                        CredentialConfigurationIds::try_new(credential_configuration_id_set.into_iter().collect())
                            .map_err(|_| MissingCredentialConfigurationIdsError)?;
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
            SendCredentialOffer {
                offer_id,
                delivery_method,
            } => {
                let form_url_encoded_credential_offer = self
                    .form_url_encoded_credential_offer
                    .as_ref()
                    .ok_or_else(|| MissingCredentialOfferError)?
                    .clone();

                match delivery_method {
                    DeliveryMethod::TargetUrl { target_url } => {
                        let client = reqwest::Client::new();
                        let target = form_url_encoded_credential_offer
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
                    DeliveryMethod::Email { recipient_email } => {
                        info!("Sending credential offer via email to: {}", recipient_email);

                        // TODO: Remove this client-side logic.
                        let offer_link = config()
                            .application_url
                            .join(&format!("offer/{}", offer_id))
                            .expect("Failed to construct offer link URL");

                        Ok(vec![CredentialOfferEmailSent {
                            offer_id,
                            recipient_email,
                            form_url_encoded_credential_offer,
                            offer_link,
                            status: Status::Pending,
                        }])
                    }
                }
            }
            VerifyCredentialRequest {
                offer_id,
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_request,
            } => {
                let credential_offer = self
                    .credential_offer
                    .clone()
                    .ok_or_else(|| MissingCredentialOfferError)?;

                let request_configuration_id =
                    if let CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId(
                        request_configuration_id,
                    ) = credential_request.credential_identifier_or_credential_configuration_id
                    {
                        request_configuration_id
                    } else {
                        return Err(UnsupportedCredentialIdentifierError);
                    };

                // Validate that the requested Credential Offer ID is contained in the Credential Offer.
                if let CredentialOffer::CredentialOffer(credential_offer) = credential_offer {
                    if !credential_offer
                        .credential_configuration_ids
                        .contains(&request_configuration_id)
                    {
                        return Err(UnknownCredentialConfiguration(request_configuration_id));
                    }
                }

                let credential_issuer = CredentialIssuer {
                    subject: services.issuer.clone(),
                    metadata: *credential_issuer_metadata,
                    authorization_server_metadata: *authorization_server_metadata,
                };

                let validated_proofs = credential_issuer
                    .validate_proofs(
                        credential_request.proofs.ok_or(MissingProofError)?,
                        Validator::Subject(services.issuer.clone()),
                    )
                    .await
                    .map_err(|e| InvalidProofError(e.to_string()))?;

                let subject_did = validated_proofs
                    .first()
                    .and_then(|proof| proof.rfc7519_claims.iss().as_ref().cloned());

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
                grant_types,
                credential_offer,
                credential_offer_uri,
                pre_authorized_code,
                status,
                tx_code,
                delivery_options: _,
            } => {
                self.offer_id = offer_id;
                self.grant_types = grant_types;
                self.credential_offer.replace(credential_offer);
                self.credential_offer_uri.replace(credential_offer_uri);
                self.pre_authorized_code = pre_authorized_code;
                self.status = status;
                self.tx_code = tx_code;
                self.delivery_options = None;
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
            CredentialOfferEmailSent { .. } => {}
            CredentialRequestVerified { subject_id, .. } => {
                self.subject_id = subject_id;
            }
            CredentialResponseCreated {
                credential_response, ..
            } => {
                self.credential_response.replace(credential_response);
            }
            TxCodeGenerated { tx_code, .. } => {
                self.tx_code.replace(tx_code);
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
    #[allow(clippy::too_many_arguments)]
    async fn test_create_offer(
        offer_id: String,
        grant_types: Vec<GrantType>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer {
                offer_id: offer_id.clone(),
                credential_configuration_ids: vec!["UniversityDegree".to_string()],
                grant_types: grant_types.clone(),
                tx_code_constraints: None,
                delivery_options: None,
            })
            .then_expect_events(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    grant_types,
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    status: Status::Created,
                    tx_code: None,
                    delivery_options: None,
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
    async fn test_create_offer_with_delivery_options(
        offer_id: String,
        grant_types: Vec<GrantType>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        delivery_options: DeliveryOptions,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given_no_previous_events()
            .when(OfferCommand::CreateCredentialOffer {
                offer_id: offer_id.clone(),
                credential_configuration_ids: vec!["UniversityDegree".to_string()],
                grant_types: grant_types.clone(),
                tx_code_constraints: None,
                delivery_options: Some(delivery_options.clone()),
            })
            .then_expect_events(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    grant_types,
                    credential_offer,
                    credential_offer_uri,
                    pre_authorized_code,
                    status: Status::Created,
                    tx_code: None,
                    delivery_options: Some(delivery_options.clone()),
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
        grant_types: Vec<GrantType>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given(vec![OfferEvent::CredentialOfferCreated {
                offer_id: offer_id.clone(),
                grant_types,
                credential_offer_uri,
                credential_offer: credential_offer.clone(),
                pre_authorized_code,
                status: Status::Created,
                tx_code: None,
                delivery_options: None,
            }])
            .when(OfferCommand::AddCredentials {
                offer_id: offer_id.clone(),
                credential_ids: vec!["credential-id".to_string()],
                credential_configuration_ids: vec!["UniversityDegree".to_string()],
            })
            .then_expect_events(vec![
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer.clone(),
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id,
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
            ]);
    }

    #[allow(clippy::too_many_arguments)]
    #[rstest]
    #[serial_test::serial]
    async fn test_verify_credential_response(
        offer_id: String,
        grant_types: Vec<GrantType>,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        #[future(awt)] university_degree_credential_request: CredentialRequest,
        credential_issuer_metadata: Box<CredentialIssuerMetadata>,
        authorization_server_metadata: Box<AuthorizationServerMetadata>,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    grant_types,
                    credential_offer: credential_offer.clone(),
                    credential_offer_uri,
                    pre_authorized_code,
                    status: Status::Created,
                    tx_code: None,
                    delivery_options: None,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer.clone(),
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
            ])
            .when(OfferCommand::VerifyCredentialRequest {
                offer_id: offer_id.clone(),
                credential_issuer_metadata,
                authorization_server_metadata,
                credential_request: university_degree_credential_request,
            })
            .then_expect_events(vec![OfferEvent::CredentialRequestVerified {
                offer_id: offer_id.clone(),
                subject_id: Some(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap()),
            }]);
    }

    #[rstest]
    #[allow(clippy::too_many_arguments)]
    #[serial_test::serial]
    async fn test_create_credential_response(
        offer_id: String,
        grant_types: Vec<GrantType>,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        credential_response: CredentialResponse,
        notification_id: String,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    grant_types,
                    credential_offer: credential_offer.clone(),
                    credential_offer_uri,
                    pre_authorized_code,
                    status: Status::Created,
                    tx_code: None,
                    delivery_options: None,
                },
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer.clone(),
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: offer_id.clone(),
                    subject_id: Some(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap()),
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
        grant_types: Vec<GrantType>,
        #[future(awt)] holder: Arc<dyn Subject>,
        #[future(awt)] pre_authorized_code: String,
        #[future(awt)] credential_offer: CredentialOffer,
        #[future(awt)] credential_offer_uri: CredentialOffer,
        #[future(awt)] form_url_encoded_credential_offer: String,
        credential_response: CredentialResponse,
        notification_id: String,
    ) {
        OfferTestFramework::with(IssuanceServices::default().await)
            .given(vec![
                OfferEvent::CredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    grant_types,
                    credential_offer: credential_offer.clone(),
                    credential_offer_uri,
                    pre_authorized_code,
                    status: Status::Created,
                    tx_code: None,
                    delivery_options: None,
                },
                OfferEvent::FormUrlEncodedCredentialOfferCreated {
                    offer_id: offer_id.clone(),
                    form_url_encoded_credential_offer,
                    status: Status::Pending,
                },
                OfferEvent::CredentialRequestVerified {
                    offer_id: offer_id.clone(),
                    subject_id: Some(holder.identifier("did:key", Algorithm::EdDSA).await.unwrap()),
                },
                // Credentials are only added after the credential request is verified (JIT)
                OfferEvent::CredentialsAdded {
                    offer_id: offer_id.clone(),
                    credential_ids: vec!["credential-id".to_string()],
                    credential_offer: credential_offer.clone(),
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
    use oid4vci::credential_request::CredentialIdentifierOrCredentialConfigurationId::CredentialConfigurationId;
    use oid4vci::proof::ProofType;
    use oid4vci::proofs::Proofs;
    use oid4vci::Proof;
    use oid4vci::{
        credential_issuer::credential_issuer_metadata::CredentialIssuerMetadata, credential_request::CredentialRequest,
    };
    use once_cell::sync::OnceCell;
    pub use rstest::*;
    use url::Url;

    static PRE_AUTHORIZED_CODE: OnceCell<String> = OnceCell::new();

    #[fixture]
    pub async fn pre_authorized_code() -> String {
        PRE_AUTHORIZED_CODE.get_or_init(generate_random_string).clone()
    }

    #[fixture]
    pub fn delivery_options() -> DeliveryOptions {
        DeliveryOptions {
            recipient_email: Some("testemail@test.com".to_string()),
        }
    }

    #[fixture]
    pub async fn holder() -> Arc<dyn oid4vc_core::Subject> {
        Arc::new(agent_secret_manager::subject::Subject::test_subject().await)
    }

    #[fixture]
    pub async fn credential_offer(
        #[future(awt)] pre_authorized_code: String,
        static_issuer_url: Url,
    ) -> CredentialOffer {
        CredentialOffer::CredentialOffer(Box::new(CredentialOfferParameters {
            credential_issuer: static_issuer_url,
            credential_configuration_ids: CredentialConfigurationIds::try_new(vec!["UniversityDegree".to_string()])
                .expect("Credential_configuration_ids should not be empty when creating a credential offer"),
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
        format!("openid-credential-offer://?credential_offer=%7B%22credential_issuer%22%3A%22https%3A%2F%2Fmy-domain.example.org%2F%22%2C%22credential_configuration_ids%22%3A%5B%22UniversityDegree%22%5D%2C%22grants%22%3A%7B%22urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Apre-authorized_code%22%3A%7B%22pre-authorized_code%22%3A%22{pre_authorized_code}%22%7D%7D%7D")
    }

    #[fixture]
    pub fn grant_types() -> Vec<GrantType> {
        vec![GrantType::PreAuthorizedCode]
    }

    #[fixture]
    pub fn offer_id() -> String {
        "offer_id".to_string()
    }

    #[fixture]
    pub async fn proof(#[future(awt)] holder: Arc<dyn Subject>, static_issuer_url: Url) -> String {
        let generated_proof = Proof::builder()
            .proof_type(ProofType::Jwt)
            .algorithm(Algorithm::EdDSA)
            .signer(holder.clone())
            .iss(
                holder
                    .identifier("did:key", Algorithm::EdDSA)
                    .await
                    .expect("Failed to get holder identifier"),
            )
            .aud(static_issuer_url.to_string())
            .iat(1571324800)
            .subject_syntax_type("did:key")
            .build()
            .await
            .expect("Failed to build proof");

        let jwt_string = match generated_proof {
            Proof::Jwt { jwt } => {
                assert!(!jwt.is_empty(), "Generated JWT should not be empty");
                jwt
            }
        };

        jwt_string
    }

    #[fixture]
    pub async fn credential_request(
        credential_configuration_id: String,
        #[future(awt)] proof: String,
    ) -> CredentialRequest {
        let generated_proof = Proof::builder()
            .proof_type(ProofType::Jwt)
            .algorithm(Algorithm::EdDSA)
            .signer(holder.clone())
            .iss(
                holder
                    .identifier("did:key", Algorithm::EdDSA)
                    .await
                    .expect("Failed to get holder identifier"),
            )
            .aud(static_issuer_url.to_string())
            .iat(1571324800)
            .subject_syntax_type("did:key")
            .build()
            .await
            .expect("Failed to build proof");

        let jwt_string = match generated_proof {
            Proof::Jwt { jwt } => {
                assert!(!jwt.is_empty(), "Generated JWT should not be empty");
                jwt
            }
        };

        CredentialRequest {
            credential_identifier_or_credential_configuration_id: CredentialConfigurationId(
                credential_configuration_id,
            ),
            proofs: Some(Proofs { jwt: vec![proof] }),
        }
    }

    #[fixture]
    pub async fn university_degree_credential_request(#[future(awt)] proof: String) -> CredentialRequest {
        CredentialRequest {
            credential_identifier_or_credential_configuration_id: CredentialConfigurationId(
                "UniversityDegree".to_string(),
            ),
            proofs: Some(Proofs { jwt: vec![proof] }),
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
