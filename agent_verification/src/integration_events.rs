use crate::authorization_request::aggregate::AuthorizationRequest;
use crate::authorization_request::event::AuthorizationRequestEvent;
use cqrs_es::EventEnvelope;
use oid4vp::token::vp_token_validator::DecodedVpToken;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

/// Public integration events emitted by the core presentation verification bounded context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, IntegrationEvent)]
#[serde(untagged)]
#[integration_event(source = "/services/verification")]
pub enum VerificationIntegrationEvent {
    /// OID4VP presentation exchange requested from holder.
    #[integration_event(rename = "com.impierce.unicore.verification.presentation.requested")]
    PresentationRequested {
        /// Unique identifier of the authorization request.
        #[integration_event(subject)]
        request_id: String,
        /// Relying party `client_id` associated with the request.
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
    },
    /// Cryptographic presentation verified successfully.
    #[integration_event(rename = "com.impierce.unicore.verification.presentation.verified")]
    PresentationVerified {
        /// Unique identifier of the authorization request.
        #[integration_event(subject)]
        request_id: String,
        /// Whether the cryptographic presentation validation succeeded (`true`).
        validated: bool,
        /// Optional OAuth2 `state` value echoed from the authorization response.
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        /// Decoded verifiable presentations extracted from an OID4VP `vp_token` response.
        #[serde(skip_serializing_if = "Option::is_none")]
        presentations: Option<DecodedVpToken>,
    },
    /// Presentation verification failed or rejected.
    #[integration_event(rename = "com.impierce.unicore.verification.presentation.failed")]
    PresentationFailed {
        /// Unique identifier of the authorization request.
        #[integration_event(subject)]
        request_id: String,
        /// Human-readable reason for the verification failure.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl TryFrom<(&AuthorizationRequestEvent, &str)> for VerificationIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from((event, request_id): (&AuthorizationRequestEvent, &str)) -> Result<Self, Self::Error> {
        match event {
            AuthorizationRequestEvent::AuthorizationRequestCreated { authorization_request } => {
                Ok(Self::PresentationRequested {
                    request_id: request_id.to_string(),
                    client_id: Some(authorization_request.client_id()),
                })
            }
            AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified { state, validated, .. } => {
                if *validated {
                    Ok(Self::PresentationVerified {
                        request_id: request_id.to_string(),
                        validated: true,
                        state: state.clone(),
                        presentations: None,
                    })
                } else {
                    Ok(Self::PresentationFailed {
                        request_id: request_id.to_string(),
                        reason: Some("Verification failed".to_string()),
                    })
                }
            }
            AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified {
                vp_token,
                state,
                validated,
            } => {
                if *validated {
                    Ok(Self::PresentationVerified {
                        request_id: request_id.to_string(),
                        validated: true,
                        state: state.clone(),
                        presentations: Some(vp_token.clone()),
                    })
                } else {
                    Ok(Self::PresentationFailed {
                        request_id: request_id.to_string(),
                        reason: Some("Verification failed".to_string()),
                    })
                }
            }
            _ => Err(UnmappedEvent),
        }
    }
}

impl ProjectIntegrationEvent for AuthorizationRequest {
    type IntegrationEvent = VerificationIntegrationEvent;

    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        VerificationIntegrationEvent::try_from((&envelope.payload, envelope.aggregate_id.as_str()))
            .ok()?
            .into_projected_cloud_event(envelope)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn try_from_siopv2_verified() {
        let event = AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
            id_token: "jwt".to_string(),
            state: None,
            validated: true,
        };
        let integration = VerificationIntegrationEvent::try_from((&event, "req-1")).expect("should map");
        assert_eq!(
            integration,
            VerificationIntegrationEvent::PresentationVerified {
                request_id: "req-1".to_string(),
                validated: true,
                state: None,
                presentations: None,
            }
        );

        let envelope = EventEnvelope {
            aggregate_id: "req-1".to_string(),
            sequence: 3,
            payload: event,
            metadata: HashMap::default(),
        };
        let projected =
            AuthorizationRequest::project_integration_event(&envelope).expect("should project SIOPv2 domain event");
        assert_eq!(projected.id, "authorization_request:req-1:3");
        assert_eq!(
            projected.event_type,
            "com.impierce.unicore.verification.presentation.verified"
        );
    }

    #[test]
    fn try_from_oid4vp_verified_includes_presentations() {
        let vp_token: DecodedVpToken = serde_json::from_value(json!({
            "verified-email": [
                {
                    "issuer": "did:web:issuer.example.com",
                    "credentialSubject": {
                        "email": "user@example.com"
                    }
                }
            ]
        }))
        .unwrap();

        let event = AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified {
            vp_token: vp_token.clone(),
            state: Some("state-123".to_string()),
            validated: true,
        };
        let integration = VerificationIntegrationEvent::try_from((&event, "req-1")).expect("should map");
        assert_eq!(
            integration,
            VerificationIntegrationEvent::PresentationVerified {
                request_id: "req-1".to_string(),
                validated: true,
                state: Some("state-123".to_string()),
                presentations: Some(vp_token),
            }
        );

        let envelope = EventEnvelope {
            aggregate_id: "req-1".to_string(),
            sequence: 2,
            payload: event,
            metadata: HashMap::default(),
        };
        let projected =
            AuthorizationRequest::project_integration_event(&envelope).expect("should project OID4VP domain event");
        assert_eq!(
            projected.data.as_ref().unwrap()["presentations"]["verified-email"][0]["credentialSubject"]["email"],
            "user@example.com"
        );
    }

    #[test]
    fn try_from_siopv2_failed() {
        let event = AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
            id_token: "jwt".to_string(),
            state: None,
            validated: false,
        };
        let integration = VerificationIntegrationEvent::try_from((&event, "req-1")).expect("should map");
        assert_eq!(
            integration,
            VerificationIntegrationEvent::PresentationFailed {
                request_id: "req-1".to_string(),
                reason: Some("Verification failed".to_string()),
            }
        );
    }
}
