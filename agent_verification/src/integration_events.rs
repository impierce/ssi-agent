use crate::authorization_request::event::AuthorizationRequestEvent;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::IntegrationEvent;

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    },
    PresentationFailed {
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl IntegrationEvent for VerificationIntegrationEvent {
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
            AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified { validated, .. }
            | AuthorizationRequestEvent::OID4VPAuthorizationResponseVerified { validated, .. } => {
                if *validated {
                    Ok(Self::PresentationVerified {
                        request_id: request_id.to_string(),
                        validated: true,
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

impl VerificationIntegrationEvent {
    /// Attempts to project an internal verification domain [`CloudEvent`] to an integration [`CloudEvent`].
    /// Returns `None` if the event type is unmapped or cannot be projected.
    pub fn project_from(domain_ce: &CloudEvent) -> Option<CloudEvent> {
        let caller_id = domain_ce.extension.callerid.clone();
        let caller_type = domain_ce.extension.callertype.clone();
        let data = domain_ce.data.as_ref()?;
        let request_id = domain_ce.subject.clone().unwrap_or_default();

        let integration_event = match domain_ce.event_type.as_str() {
            "com.impierce.unicore.authorization-request-created" => {
                if let Ok(event) = serde_json::from_value::<AuthorizationRequestEvent>(serde_json::json!({
                    "AuthorizationRequestCreated": data
                })) {
                    Self::try_from((&event, request_id.as_str())).ok()?
                } else {
                    let client_id = data
                        .get("authorization_request")
                        .and_then(|ar| ar.get("client_id").or_else(|| ar.get("client_id_scheme")))
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);

                    Self::PresentationRequested { request_id, client_id }
                }
            }
            "com.impierce.unicore.oid-4-vp-authorization-response-verified" => {
                if let Ok(event) = serde_json::from_value::<AuthorizationRequestEvent>(serde_json::json!({
                    "OID4VPAuthorizationResponseVerified": data
                })) {
                    Self::try_from((&event, request_id.as_str())).ok()?
                } else {
                    let validated = data.get("validated").and_then(|v| v.as_bool()).unwrap_or(true);
                    if validated {
                        Self::PresentationVerified {
                            request_id,
                            validated: true,
                        }
                    } else {
                        Self::PresentationFailed {
                            request_id,
                            reason: Some("Verification failed".to_string()),
                        }
                    }
                }
            }
            "com.impierce.unicore.si-o-pv-2-authorization-response-verified" => {
                if let Ok(event) = serde_json::from_value::<AuthorizationRequestEvent>(serde_json::json!({
                    "SIOPv2AuthorizationResponseVerified": data
                })) {
                    Self::try_from((&event, request_id.as_str())).ok()?
                } else {
                    let validated = data.get("validated").and_then(|v| v.as_bool()).unwrap_or(true);
                    if validated {
                        Self::PresentationVerified {
                            request_id,
                            validated: true,
                        }
                    } else {
                        Self::PresentationFailed {
                            request_id,
                            reason: Some("Verification failed".to_string()),
                        }
                    }
                }
            }
            _ => return None,
        };

        let mut result_ce = integration_event
            .into_cloud_event("/services/verification", caller_id, caller_type)
            .ok()?;

        if let Some(occurred_at) = domain_ce.time {
            result_ce.time = Some(occurred_at);
        }

        Some(result_ce)
    }
}

/// Project an internal domain [`CloudEvent`] to a [`VerificationIntegrationEvent`] if applicable.
#[must_use]
pub fn project_verification_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    VerificationIntegrationEvent::project_from(domain_ce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_siopv2_verified() {
        let event = AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
            id_token: "jwt".to_string(),
            state: None,
            validated: true,
        };
        let integration =
            VerificationIntegrationEvent::try_from((&event, "req-1")).expect("should map");
        assert_eq!(
            integration,
            VerificationIntegrationEvent::PresentationVerified {
                request_id: "req-1".to_string(),
                validated: true,
            }
        );
    }

    #[test]
    fn try_from_siopv2_failed() {
        let event = AuthorizationRequestEvent::SIOPv2AuthorizationResponseVerified {
            id_token: "jwt".to_string(),
            state: None,
            validated: false,
        };
        let integration =
            VerificationIntegrationEvent::try_from((&event, "req-1")).expect("should map");
        assert_eq!(
            integration,
            VerificationIntegrationEvent::PresentationFailed {
                request_id: "req-1".to_string(),
                reason: Some("Verification failed".to_string()),
            }
        );
    }
}
