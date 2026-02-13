use super::{command::PresentationCommand, error::PresentationError, event::PresentationEvent};
use crate::services::HolderServices;
use agent_shared::config::{get_preferred_did_method, get_preferred_signing_algorithm};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::Aggregate;
use identity_core::convert::ToJson;
use identity_credential::{credential::Jwt, presentation::JwtPresentationOptions};
use jsonwebtoken::Header;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Presentation {
    #[serde(rename = "id")]
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
                    .map_err(|err| MissingIdentifierError(err.to_string()))?;

                let mut presentation_builder = identity_credential::presentation::Presentation::builder(
                    subject_did
                        .parse::<identity_core::common::Url>()
                        .map_err(|err| InvalidUrlError(err.to_string()))?,
                    Default::default(),
                );
                for signed_credential in signed_credentials {
                    presentation_builder = presentation_builder.credential(signed_credential);
                }

                #[cfg(feature = "test_utils")]
                let options = JwtPresentationOptions::default()
                    .issuance_date(identity_core::common::Timestamp::from_unix(0).unwrap());
                #[cfg(not(feature = "test_utils"))]
                let options = JwtPresentationOptions::default();

                let verifiable_presentation: identity_credential::presentation::Presentation<Jwt> =
                    presentation_builder
                        .build()
                        .map_err(|err| PresentationBuilderError(err.to_string()))?;

                let payload = verifiable_presentation
                    .serialize_jwt(&options)
                    .map_err(|err| SerializationError(err.to_string()))?;

                // Compose JWT
                let header = Header {
                    alg: get_preferred_signing_algorithm(),
                    typ: Some("JWT".to_string()),
                    // TODO: make dynamic
                    kid: Some(format!("{subject_did}#key-0")),
                    ..Default::default()
                };

                let message = [
                    URL_SAFE_NO_PAD.encode(
                        header
                            .to_json_vec()
                            .map_err(|err| SerializationError(err.to_string()))?,
                    ),
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
                    .map_err(|err| SigningError(err.to_string()))?;
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

        debug!("Applying event: {:?}", event);

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

#[cfg(test)]
pub mod presentation_tests {

    use crate::offer::aggregate::test_utils::signed_credentials;
    use crate::offer::aggregate::OfferCredential;

    use super::test_utils::*;
    use super::*;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type PresentationTestFramework = TestFramework<Presentation>;

    #[rstest]
    #[serial_test::serial]
    async fn test_create_presentation(
        presentation_id: String,
        signed_credentials: Vec<OfferCredential>,
        signed_presentation: Jwt,
    ) {
        PresentationTestFramework::with(HolderServices::default().await)
            .given_no_previous_events()
            .when(PresentationCommand::CreatePresentation {
                presentation_id: presentation_id.clone(),
                signed_credentials: signed_credentials.into_iter().map(|c| c.credential).collect(),
            })
            .then_expect_events(vec![PresentationEvent::PresentationCreated {
                presentation_id,
                signed_presentation,
            }])
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use super::*;
    use rstest::*;

    #[fixture]
    pub fn presentation_id() -> String {
        "presentation-id".to_string()
    }

    #[fixture]
    pub fn signed_presentation() -> Jwt {
        Jwt::from("eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I2tleS0wIn0.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsIm5iZiI6MCwidnAiOnsiQGNvbnRleHQiOiJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsInR5cGUiOiJWZXJpZmlhYmxlUHJlc2VudGF0aW9uIiwidmVyaWZpYWJsZUNyZWRlbnRpYWwiOlsiZXlKMGVYQWlPaUpLVjFRaUxDSmhiR2NpT2lKRlpFUlRRU0lzSW10cFpDSTZJbVJwWkRwclpYazZlalpOYTJkRk9EUk9RMDF3VFdWQmVEbHFTemxqWmpWWE5FYzRaMk5hT1hoMWQwcDJSekZsTjNkT2F6aExRMmQwSTNvMlRXdG5SVGcwVGtOTmNFMWxRWGc1YWtzNVkyWTFWelJIT0dkaldqbDRkWGRLZGtjeFpUZDNUbXM0UzBObmRDSjkuZXlKcGMzTWlPaUprYVdRNmEyVjVPbm8yVFd0blJUZzBUa05OY0UxbFFYZzVha3M1WTJZMVZ6UkhPR2RqV2psNGRYZEtka2N4WlRkM1RtczRTME5uZENJc0luTjFZaUk2SW1ScFpEcHJaWGs2ZWtSdVlXVlNkMVEwWnpaQldrTkllbmgyVGt3M1JFeHFjVlJoVkRnNFlXMDBXRkkyVkZWSGNrdHlOa1JZYWpaVWVpSXNJbTVpWmlJNk1USTJNak13TkRBd01Dd2lhV0YwSWpveE1qWXlNekEwTURBd0xDSjJZeUk2ZXlKQVkyOXVkR1Y0ZENJNld5Sm9kSFJ3Y3pvdkwzZDNkeTUzTXk1dmNtY3ZNakF4T0M5amNtVmtaVzUwYVdGc2N5OTJNU0pkTENKMGVYQmxJanBiSWxabGNtbG1hV0ZpYkdWRGNtVmtaVzUwYVdGc0lsMHNJbU55WldSbGJuUnBZV3hUZFdKcVpXTjBJanA3SW1sa0lqb2laR2xrT210bGVUcDZSRzVoWlZKM1ZEUm5Oa0ZhUTBoNmVIWk9URGRFVEdweFZHRlVPRGhoYlRSWVVqWlVWVWR5UzNJMlJGaHFObFI2SWl3aVpHVm5jbVZsSWpwN0luUjVjR1VpT2lKTllYTjBaWEpFWldkeVpXVWlMQ0p1WVcxbElqb2lUV0Z6ZEdWeUlHOW1JRTlqWldGdWIyZHlZWEJvZVNKOUxDSm1hWEp6ZEY5dVlXMWxJam9pUm1WeWNtbHpJaXdpYkdGemRGOXVZVzFsSWpvaVVuVnpkR0ZqWldGdUluMHNJbWx6YzNWbGNpSTZJbVJwWkRwclpYazZlalpOYTJkRk9EUk9RMDF3VFdWQmVEbHFTemxqWmpWWE5FYzRaMk5hT1hoMWQwcDJSekZsTjNkT2F6aExRMmQwSWl3aWFYTnpkV0Z1WTJWRVlYUmxJam9pTWpBeE1DMHdNUzB3TVZRd01Eb3dNRG93TUZvaUxDSmpjbVZrWlc1MGFXRnNVM1JoZEhWeklqcDdJbWxrSWpvaWFIUjBjSE02THk5dGVTMWtiMjFoYVc0dVpYaGhiWEJzWlM1dmNtY3ZhV1YwWmkxdllYVjBhQzEwYjJ0bGJpMXpkR0YwZFhNdGJHbHpkQzh3SWl3aWRIbHdaU0k2SW5OMFlYUjFjMnhwYzNRcmFuZDBJaXdpYVdSNElqb3hNak1zSW5WeWFTSTZJbWgwZEhCek9pOHZiWGt0Wkc5dFlXbHVMbVY0WVcxd2JHVXViM0puTDJsbGRHWXRiMkYxZEdndGRHOXJaVzR0YzNSaGRIVnpMV3hwYzNRdk1DSjlmU3dpYzNSaGRIVnpJanA3SW5OMFlYUjFjMTlzYVhOMElqcDdJbWxrZUNJNk1USXpMQ0oxY21raU9pSm9kSFJ3Y3pvdkwyMTVMV1J2YldGcGJpNWxlR0Z0Y0d4bExtOXlaeTlwWlhSbUxXOWhkWFJvTFhSdmEyVnVMWE4wWVhSMWN5MXNhWE4wTHpBaWZYMTkuNm9WamZOMDZkelFtZDJvQ1ZCbTlsZ09raEwwbUxJSG4tSTh2VUIwT1gzbjdNYmpoSVdqamZBXzZTU3Zjb2ZHR0xqLUJGdDFmTUNEVXkxVnJpQ3ZpQUEiXX19.yC9j8CGnspTVL-wPBHaTw5s9wZ1d-bDyScPbXziAkPQkxHpjsbSkGe5bnWQp3yaZZkEs1EdKRb5wkx_3VMn2CA".to_string())
    }
}
