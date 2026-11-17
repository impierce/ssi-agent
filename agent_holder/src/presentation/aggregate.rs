use super::{command::PresentationCommand, error::PresentationError, event::PresentationEvent};
use crate::credential::signed::SignedCredential;
use crate::services::HolderServices;
use agent_shared::config::{get_preferred_did_method, get_preferred_signing_algorithm};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cqrs_es::{event_sink::EventSink, Aggregate};
use identity_kit::{
    did::Did,
    vc::{EnvelopedVc, Jwt, JwtClaims, Presentation as VerifiablePresentation, PresentationV2},
};
use jsonwebtoken::Header;
use oid4vc_core::{Sign as _, Subject as _};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
#[schema(as = PresentationView)]
pub struct Presentation {
    #[serde(rename = "id")]
    pub presentation_id: String,
    #[schema(value_type = Option<String>)]
    pub signed: Option<Jwt>,
}

impl Aggregate for Presentation {
    type Command = PresentationCommand;
    type Event = PresentationEvent;
    type Error = PresentationError;
    type Services = Arc<HolderServices>;

    const TYPE: &'static str = "presentation";

    async fn handle(
        &mut self,
        command: Self::Command,
        services: &Self::Services,
        sink: &EventSink<Self>,
    ) -> Result<(), Self::Error> {
        use PresentationCommand::*;
        use PresentationError::*;
        use PresentationEvent::*;

        let events: Vec<Self::Event> = match command {
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

                let holder_did: Did = subject_did
                    .parse()
                    .map_err(|err: identity_kit::did::DidError| InvalidUrlError(err.to_string()))?;
                let (mut claims, typ) = presentation_claims(holder_did, signed_credentials)?;
                #[cfg(feature = "test_utils")]
                {
                    claims.nbf = Some(0);
                }
                #[cfg(not(feature = "test_utils"))]
                {
                    claims.nbf = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .and_then(|now| i64::try_from(now.as_secs()).ok());
                }

                let payload = serde_json::to_vec(&claims).map_err(|err| SerializationError(err.to_string()))?;

                // The `kid` must name a verification method the DID Document actually carries, so
                // that a verifier resolving the holder's DID can find the key this is signed with.
                // It is the same lookup `identifier` above makes, kept whole instead of truncated
                // to its DID.
                let kid = holder
                    .key_id(
                        get_preferred_did_method().to_string().as_ref(),
                        get_preferred_signing_algorithm(),
                    )
                    .await
                    .ok_or(KeyIdError)?;

                // Compose JWT
                let header = Header {
                    alg: get_preferred_signing_algorithm(),
                    typ: Some(typ.to_string()),
                    kid: Some(kid),
                    ..Default::default()
                };

                let message = [
                    URL_SAFE_NO_PAD
                        .encode(serde_json::to_vec(&header).map_err(|err| SerializationError(err.to_string()))?),
                    URL_SAFE_NO_PAD.encode(payload),
                ]
                .join(".");

                let proof_value = oid4vc_core::Sign::sign(
                    &**holder,
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
                    signed_presentation: Jwt::new(message).map_err(|err| SerializationError(err.to_string()))?,
                }])
            }
        }?;

        for event in events {
            sink.write(event, self).await;
        }

        Ok(())
    }

    fn apply(&mut self, event: Self::Event) {
        use PresentationEvent::*;

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

/// The claims of a presentation of `credentials` by `holder`, and the `typ` to sign them under: credential JWTs in a
/// VCDM 1.1 presentation (`vp` claim, `JWT`), `vc+sd-jwt` credentials enveloped with every disclosure they carry in
/// a VCDM 2.0 presentation (`vp+jwt`). A presentation of both kinds is refused.
fn presentation_claims(
    holder: Did,
    credentials: Vec<SignedCredential>,
) -> Result<(JwtClaims, &'static str), PresentationError> {
    let (jwts, sd_jwts): (Vec<_>, Vec<_>) = credentials
        .into_iter()
        .partition(|credential| matches!(credential, SignedCredential::Jwt(_)));
    if sd_jwts.is_empty() {
        let presentation = jwts
            .into_iter()
            .filter_map(|credential| match credential {
                SignedCredential::Jwt(jwt) => Some(jwt),
                SignedCredential::SdJwt(_) => None,
            })
            .fold(
                VerifiablePresentation::new().with_holder(holder),
                VerifiablePresentation::with_credential,
            );
        let claims = presentation
            .to_jwt_claims()
            .map_err(|err| PresentationError::PresentationBuilderError(err.to_string()))?;
        return Ok((claims, "JWT"));
    }
    if !jwts.is_empty() {
        return Err(PresentationError::MixedCredentialFormatsError);
    }
    let presentation = sd_jwts
        .into_iter()
        .filter_map(|credential| match credential {
            SignedCredential::SdJwt(sd_jwt) => Some(EnvelopedVc::from_sd_jwt(&sd_jwt)),
            SignedCredential::Jwt(_) => None,
        })
        .fold(
            PresentationV2::new().with_holder(holder.clone()),
            PresentationV2::with_credential,
        );
    Ok((
        presentation.to_jwt_claims().with_iss(holder.as_str()),
        PresentationV2::JWT_TYP,
    ))
}

#[cfg(test)]
pub mod presentation_tests {

    use crate::offer::aggregate::test_utils::signed_credentials;
    use crate::offer::aggregate::OfferCredential;

    use super::test_utils::*;
    use super::*;
    use crate::credential::signed::issue_vc_sd_jwt;
    use agent_secret_manager::service::Service;
    use cqrs_es::test::TestFramework;
    use rstest::rstest;

    type PresentationTestFramework = TestFramework<Presentation>;

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_presentation(
        presentation_id: String,
        signed_credentials: Vec<OfferCredential>,
        signed_presentation: Jwt,
    ) {
        PresentationTestFramework::with(HolderServices::default().await)
            .given_no_previous_events()
            .when_async(PresentationCommand::CreatePresentation {
                presentation_id: presentation_id.clone(),
                signed_credentials: signed_credentials.into_iter().map(|c| c.credential).collect(),
            })
            .await
            .then_expect_events(vec![PresentationEvent::PresentationCreated {
                presentation_id,
                signed_presentation,
            }])
    }

    /// `vc+sd-jwt` credentials are presented in a VCDM 2.0 presentation (`vp+jwt`), each enveloped with every
    /// disclosure it carries.
    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_presentation_of_sd_jwt_credentials(presentation_id: String) {
        let services = HolderServices::default().await;
        let sd_jwt = issue_vc_sd_jwt(&services.holder, chrono::Utc::now() + chrono::Days::new(1)).await;
        let holder = services
            .holder
            .identifier("did:key", jsonwebtoken::Algorithm::EdDSA)
            .await
            .unwrap();

        let events = PresentationTestFramework::with(services)
            .given_no_previous_events()
            .when_async(PresentationCommand::CreatePresentation {
                presentation_id: presentation_id.clone(),
                signed_credentials: vec![SignedCredential::SdJwt(sd_jwt.clone())],
            })
            .await
            .inspect_result()
            .unwrap();
        let [PresentationEvent::PresentationCreated {
            presentation_id: created,
            signed_presentation,
        }] = events.as_slice()
        else {
            panic!("one presentation created");
        };
        assert_eq!(*created, presentation_id);

        let decoded = identity_kit::vc::validation::decode_presentation_v2(signed_presentation).unwrap();
        assert_eq!(decoded.header.typ.as_deref(), Some(PresentationV2::JWT_TYP));
        assert_eq!(decoded.claims.iss.as_deref(), Some(holder.as_str()));
        assert_eq!(decoded.holder().map(|holder| holder.as_str()), Some(holder.as_str()));
        assert_eq!(
            decoded.presentation.verifiable_credential,
            vec![EnvelopedVc::from_sd_jwt(&sd_jwt)]
        );
        assert_eq!(
            decoded.credentials[0].credential().subject().unwrap().properties["name"],
            "Example LLC"
        );
    }

    #[rstest]
    #[serial_test::serial]
    #[tokio::test]
    async fn test_create_presentation_of_mixed_credential_formats(
        presentation_id: String,
        signed_credentials: Vec<OfferCredential>,
    ) {
        let services = HolderServices::default().await;
        let sd_jwt = issue_vc_sd_jwt(&services.holder, chrono::Utc::now() + chrono::Days::new(1)).await;
        let mut credentials: Vec<_> = signed_credentials.into_iter().map(|c| c.credential).collect();
        credentials.push(SignedCredential::SdJwt(sd_jwt));

        PresentationTestFramework::with(services)
            .given_no_previous_events()
            .when_async(PresentationCommand::CreatePresentation {
                presentation_id,
                signed_credentials: credentials,
            })
            .await
            .then_expect_error_message(&PresentationError::MixedCredentialFormatsError.to_string());
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

    /// The `kid` names the holder's verification method as the DID Document carries it, so a
    /// verifier can resolve the key this is signed with. A `kid` that resolves to nothing fails
    /// VP validation; decodable embedded credentials remain available as unverified data.
    #[fixture]
    pub fn signed_presentation() -> Jwt {
        "eyJ0eXAiOiJKV1QiLCJhbGciOiJFZERTQSIsImtpZCI6ImRpZDprZXk6ejZNa2dFODROQ01wTWVBeDlqSzljZjVXNEc4Z2NaOXh1d0p2RzFlN3dOazhLQ2d0I3o2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCJ9.eyJpc3MiOiJkaWQ6a2V5Ono2TWtnRTg0TkNNcE1lQXg5aks5Y2Y1VzRHOGdjWjl4dXdKdkcxZTd3Tms4S0NndCIsIm5iZiI6MCwidnAiOnsiQGNvbnRleHQiOiJodHRwczovL3d3dy53My5vcmcvMjAxOC9jcmVkZW50aWFscy92MSIsInR5cGUiOiJWZXJpZmlhYmxlUHJlc2VudGF0aW9uIiwidmVyaWZpYWJsZUNyZWRlbnRpYWwiOlsiZXlKMGVYQWlPaUpLVjFRaUxDSmhiR2NpT2lKRlpFUlRRU0lzSW10cFpDSTZJbVJwWkRwclpYazZlalpOYTJkRk9EUk9RMDF3VFdWQmVEbHFTemxqWmpWWE5FYzRaMk5hT1hoMWQwcDJSekZsTjNkT2F6aExRMmQwSTNvMlRXdG5SVGcwVGtOTmNFMWxRWGc1YWtzNVkyWTFWelJIT0dkaldqbDRkWGRLZGtjeFpUZDNUbXM0UzBObmRDSjkuZXlKcGMzTWlPaUprYVdRNmEyVjVPbm8yVFd0blJUZzBUa05OY0UxbFFYZzVha3M1WTJZMVZ6UkhPR2RqV2psNGRYZEtka2N4WlRkM1RtczRTME5uZENJc0luTjFZaUk2SW1ScFpEcHJaWGs2ZWtSdVlXVlNkMVEwWnpaQldrTkllbmgyVGt3M1JFeHFjVlJoVkRnNFlXMDBXRkkyVkZWSGNrdHlOa1JZYWpaVWVpSXNJbTVpWmlJNk1USTJNak13TkRBd01Dd2lhV0YwSWpveE1qWXlNekEwTURBd0xDSjJZeUk2ZXlKQVkyOXVkR1Y0ZENJNld5Sm9kSFJ3Y3pvdkwzZDNkeTUzTXk1dmNtY3ZNakF4T0M5amNtVmtaVzUwYVdGc2N5OTJNU0pkTENKMGVYQmxJanBiSWxabGNtbG1hV0ZpYkdWRGNtVmtaVzUwYVdGc0lsMHNJbU55WldSbGJuUnBZV3hUZFdKcVpXTjBJanA3SW1sa0lqb2laR2xrT210bGVUcDZSRzVoWlZKM1ZEUm5Oa0ZhUTBoNmVIWk9URGRFVEdweFZHRlVPRGhoYlRSWVVqWlVWVWR5UzNJMlJGaHFObFI2SWl3aVpHVm5jbVZsSWpwN0luUjVjR1VpT2lKTllYTjBaWEpFWldkeVpXVWlMQ0p1WVcxbElqb2lUV0Z6ZEdWeUlHOW1JRTlqWldGdWIyZHlZWEJvZVNKOUxDSm1hWEp6ZEY5dVlXMWxJam9pUm1WeWNtbHpJaXdpYkdGemRGOXVZVzFsSWpvaVVuVnpkR0ZqWldGdUluMHNJbWx6YzNWbGNpSTZJbVJwWkRwclpYazZlalpOYTJkRk9EUk9RMDF3VFdWQmVEbHFTemxqWmpWWE5FYzRaMk5hT1hoMWQwcDJSekZsTjNkT2F6aExRMmQwSWl3aWFYTnpkV0Z1WTJWRVlYUmxJam9pTWpBeE1DMHdNUzB3TVZRd01Eb3dNRG93TUZvaUxDSmpjbVZrWlc1MGFXRnNVM1JoZEhWeklqcDdJbWxrSWpvaWFIUjBjSE02THk5dGVTMWtiMjFoYVc0dVpYaGhiWEJzWlM1dmNtY3ZhV1YwWmkxdllYVjBhQzEwYjJ0bGJpMXpkR0YwZFhNdGJHbHpkQzh3SWl3aWRIbHdaU0k2SW5OMFlYUjFjMnhwYzNRcmFuZDBJaXdpYVdSNElqb3hNak1zSW5WeWFTSTZJbWgwZEhCek9pOHZiWGt0Wkc5dFlXbHVMbVY0WVcxd2JHVXViM0puTDJsbGRHWXRiMkYxZEdndGRHOXJaVzR0YzNSaGRIVnpMV3hwYzNRdk1DSjlmU3dpYzNSaGRIVnpJanA3SW5OMFlYUjFjMTlzYVhOMElqcDdJbWxrZUNJNk1USXpMQ0oxY21raU9pSm9kSFJ3Y3pvdkwyMTVMV1J2YldGcGJpNWxlR0Z0Y0d4bExtOXlaeTlwWlhSbUxXOWhkWFJvTFhSdmEyVnVMWE4wWVhSMWN5MXNhWE4wTHpBaWZYMTkuNm9WamZOMDZkelFtZDJvQ1ZCbTlsZ09raEwwbUxJSG4tSTh2VUIwT1gzbjdNYmpoSVdqamZBXzZTU3Zjb2ZHR0xqLUJGdDFmTUNEVXkxVnJpQ3ZpQUEiXX19.zJ6ieLFJ1LWozlV7y8LuYTF1EtWR1rtaHEqpBFaj2gkW9hDU7ha9nhJiuOwUKwK5Jl-H4SsyYEmavnSmsasYDQ".parse().unwrap()
    }
}
