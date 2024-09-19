use super::{command::PresentationCommand, error::PresentationError, event::PresentationEvent};
use crate::services::HolderServices;
use agent_shared::config::{get_preferred_did_method, get_preferred_signing_algorithm};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use identity_credential::credential::Jwt;
use jsonwebtoken::{Algorithm, Header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Presentation {
    pub presentation_id: String,
    pub signed: Option<Jwt>,
}

#[async_trait]
impl Aggregate for Presentation {
    type Command = PresentationCommand;
    type Event = PresentationEvent;
    type Error = PresentationError;
    type Services = Arc<HolderServices>;

    fn aggregate_type() -> String {
        "presentation".to_string()
    }

    async fn handle(&self, command: Self::Command, services: &Self::Services) -> Result<Vec<Self::Event>, Self::Error> {
        use PresentationCommand::*;
        use PresentationError::*;
        use PresentationEvent::*;

        info!("Handling command: {:?}", command);

        match command {
            CreatePresentation {
                presentation_id,
                signed_credentials,
            } => {
                let holder = &services.holder;
                let subject_did = holder
                    .identifier(
                        get_preferred_did_method().to_string().as_ref(),
                        get_preferred_signing_algorithm(),
                    )
                    .await
                    .unwrap();

                let mut presentation_builder = identity_credential::presentation::Presentation::builder(
                    subject_did.parse().unwrap(),
                    Default::default(),
                );
                for signed_credential in signed_credentials {
                    presentation_builder = presentation_builder.credential(signed_credential);
                }

                let verifiable_presentation: identity_credential::presentation::Presentation<Jwt> =
                    presentation_builder.build().unwrap();

                let payload = verifiable_presentation
                    .serialize_jwt(&Default::default())
                    .expect("FIX THISS");

                // Compose JWT
                let header = Header {
                    alg: Algorithm::ES256,
                    typ: Some("JWT".to_string()),
                    kid: Some(format!("{subject_did}#key-0")),
                    ..Default::default()
                };

                let message = [
                    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap().as_slice()),
                    // FIX THISS?
                    URL_SAFE_NO_PAD.encode(payload.as_bytes()),
                ]
                .join(".");

                let proof_value = holder
                    .sign(
                        &message,
                        get_preferred_did_method().to_string().as_ref(),
                        get_preferred_signing_algorithm(),
                    )
                    .await
                    .unwrap();
                let signature = URL_SAFE_NO_PAD.encode(proof_value.as_slice());
                let message = [message, signature].join(".");

                Ok(vec![PresentationCreated {
                    presentation_id,
                    signed_presentation: Jwt::from(message),
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        use PresentationEvent::*;

        info!("Applying event: {:?}", event);

        match event {
            PresentationCreated {
                presentation_id,
                signed_presentation,
            } => {
                self.presentation_id = presentation_id;
                self.signed.replace(signed_presentation);
            }
        }
    }
}
