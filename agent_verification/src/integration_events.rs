use crate::authorization_request::aggregate::AuthorizationRequest;
use crate::authorization_request::event::AuthorizationRequestEvent;
use cqrs_es::EventEnvelope;
use oid4vp::token::vp_token_validator::DecodedVpToken;
use serde::Serialize;
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum VerificationIntegrationEvent {
    PresentationRequested {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
    },
    PresentationVerified {
        request_id: String,
        validated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        presentations: Option<DecodedVpToken>,
    },
    PresentationFailed {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl IntegrationEvent for VerificationIntegrationEvent {
    const SOURCE: &'static str = "/services/verification";

    fn event_type(&self) -> &'static str {
        match self {
            Self::PresentationRequested { .. } => "com.impierce.unicore.verification.presentation.requested",
            Self::PresentationVerified { .. } => "com.impierce.unicore.verification.presentation.verified",
            Self::PresentationFailed { .. } => "com.impierce.unicore.verification.presentation.failed",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::PresentationRequested { request_id, .. }
            | Self::PresentationVerified { request_id, .. }
            | Self::PresentationFailed { request_id, .. } => Some(request_id.clone()),
        }
    }
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
            .into_projected_cloud_event(envelope, "/services/verification")
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
