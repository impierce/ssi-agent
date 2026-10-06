use crate::credential::event::CredentialEvent;
use crate::offer::event::OfferEvent;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::IntegrationEvent;

/// Marker error indicating that a domain event has no public integration event mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnmappedEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IssuanceIntegrationEvent {
    CredentialOffered {
        offer_id: String,
        credential_ids: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pre_authorized_code: Option<String>,
    },
    CredentialIssued {
        credential_id: String,
        status: String,
    },
    CredentialRevoked {
        credential_id: String,
    },
}

impl IntegrationEvent for IssuanceIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::CredentialOffered { .. } => "com.impierce.unicore.issuance.credential.offered",
            Self::CredentialIssued { .. } => "com.impierce.unicore.issuance.credential.issued",
            Self::CredentialRevoked { .. } => "com.impierce.unicore.issuance.credential.revoked",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::CredentialOffered { offer_id, .. } => Some(offer_id.clone()),
            Self::CredentialIssued { credential_id, .. } | Self::CredentialRevoked { credential_id } => {
                Some(credential_id.clone())
            }
        }
    }
}

impl TryFrom<OfferEvent> for IssuanceIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: OfferEvent) -> Result<Self, Self::Error> {
        match event {
            OfferEvent::CredentialOfferCreated {
                offer_id,
                pre_authorized_code,
                credential_offer,
                ..
            } => {
                let credential_ids = match credential_offer {
                    oid4vci::credential_offer::CredentialOffer::CredentialOffer(payload) => payload
                        .credential_configuration_ids
                        .iter()
                        .cloned()
                        .collect(),
                    _ => Vec::new(),
                };

                Ok(Self::CredentialOffered {
                    offer_id,
                    credential_ids,
                    pre_authorized_code: Some(pre_authorized_code),
                })
            }
            _ => Err(UnmappedEvent),
        }
    }
}

impl TryFrom<CredentialEvent> for IssuanceIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: CredentialEvent) -> Result<Self, Self::Error> {
        match event {
            CredentialEvent::SignedCredentialCreated { credential_id, .. } => {
                Ok(Self::CredentialIssued {
                    credential_id,
                    status: "issued".to_string(),
                })
            }
            CredentialEvent::CredentialSigned {
                credential_id,
                status,
                ..
            } => {
                let status_str = match status {
                    crate::credential::aggregate::Status::Pending => "pending",
                    crate::credential::aggregate::Status::Issued => "issued",
                };
                Ok(Self::CredentialIssued {
                    credential_id,
                    status: status_str.to_string(),
                })
            }
            CredentialEvent::CredentialStatusUpdated { credential_id, .. } => {
                Ok(Self::CredentialRevoked { credential_id })
            }
            _ => Err(UnmappedEvent),
        }
    }
}

impl IssuanceIntegrationEvent {
    /// Translates an internal issuance domain [`CloudEvent`] to an [`IssuanceIntegrationEvent`].
    #[must_use]
    pub fn project_from(domain_ce: &CloudEvent) -> Option<CloudEvent> {
        let data = domain_ce.data.as_ref()?;
        let caller_id = domain_ce.extension.callerid.clone();
        let caller_type = domain_ce.extension.callertype.clone();

        let integration_event = match domain_ce.event_type.as_str() {
            "com.impierce.unicore.credential-offer-created" => {
                if let Ok(event) = serde_json::from_value::<OfferEvent>(serde_json::json!({
                    "CredentialOfferCreated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let offer_id = data
                        .get("offer_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let credential_ids = data
                        .get("credential_ids")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(ToString::to_string)).collect())
                        .unwrap_or_default();
                    let pre_authorized_code = data
                        .get("pre_authorized_code")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);

                    Self::CredentialOffered {
                        offer_id,
                        credential_ids,
                        pre_authorized_code,
                    }
                }
            }
            "com.impierce.unicore.signed-credential-created" => {
                if let Ok(event) = serde_json::from_value::<CredentialEvent>(serde_json::json!({
                    "SignedCredentialCreated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let credential_id = data
                        .get("credential_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let status = data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("issued")
                        .to_string();

                    Self::CredentialIssued { credential_id, status }
                }
            }
            "com.impierce.unicore.credential-signed" => {
                if let Ok(event) = serde_json::from_value::<CredentialEvent>(serde_json::json!({
                    "CredentialSigned": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let credential_id = data
                        .get("credential_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();
                    let status = data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("issued")
                        .to_string();

                    Self::CredentialIssued { credential_id, status }
                }
            }
            "com.impierce.unicore.credential-status-updated" => {
                if let Ok(event) = serde_json::from_value::<CredentialEvent>(serde_json::json!({
                    "CredentialStatusUpdated": data
                })) {
                    Self::try_from(event).ok()?
                } else {
                    let credential_id = data
                        .get("credential_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string)
                        .or_else(|| domain_ce.subject.clone())
                        .unwrap_or_default();

                    Self::CredentialRevoked { credential_id }
                }
            }
            _ => return None,
        };

        let mut result_ce = integration_event
            .into_cloud_event("/services/issuance", caller_id, caller_type)
            .ok()?;

        if let Some(occurred_at) = domain_ce.time {
            result_ce.time = Some(occurred_at);
        }

        Some(result_ce)
    }
}

/// Project an internal domain [`CloudEvent`] to an [`IssuanceIntegrationEvent`] if applicable.
#[must_use]
pub fn project_issuance_event(domain_ce: &CloudEvent) -> Option<CloudEvent> {
    IssuanceIntegrationEvent::project_from(domain_ce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::event::CredentialEvent;

    #[test]
    fn try_from_signed_credential_created() {
        let event = CredentialEvent::SignedCredentialCreated {
            credential_id: "cred-1".to_string(),
            signed_credential: serde_json::json!({}),
            notification_id: None,
        };
        let integration = IssuanceIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            IssuanceIntegrationEvent::CredentialIssued {
                credential_id: "cred-1".to_string(),
                status: "issued".to_string(),
            }
        );
    }

    #[test]
    fn try_from_credential_revoked() {
        let event = CredentialEvent::CredentialStatusUpdated {
            credential_id: "cred-2".to_string(),
            credential_status: crate::credential::aggregate::CredentialStatus::default(),
        };
        let integration = IssuanceIntegrationEvent::try_from(event).expect("should map");
        assert_eq!(
            integration,
            IssuanceIntegrationEvent::CredentialRevoked {
                credential_id: "cred-2".to_string(),
            }
        );
    }
}
