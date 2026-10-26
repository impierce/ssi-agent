use crate::credential::aggregate::Credential;
use crate::credential::event::CredentialEvent;
use crate::offer::aggregate::{DeliveryOptions, Offer};
use crate::offer::event::OfferEvent;
use crate::reissuance::aggregate::Reissuance;
use crate::reissuance::event::ReissuanceEvent;
use cqrs_es::EventEnvelope;
use serde::{Deserialize, Serialize};
use shared_kernel::event_bus::CloudEvent;
use shared_kernel::{IntegrationEvent, ProjectIntegrationEvent};
use url::Url;

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
        #[serde(skip_serializing_if = "Option::is_none")]
        tx_code: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery_options: Option<DeliveryOptions>,
    },
    TxCodeGenerated {
        offer_id: String,
        tx_code: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery_options: Option<DeliveryOptions>,
    },
    CredentialOfferEmailSent {
        offer_id: String,
        recipient_email: String,
        form_url_encoded_credential_offer: String,
        offer_link: Url,
    },
    CredentialRequestVerified {
        offer_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subject_id: Option<String>,
    },
    CredentialIssued {
        credential_id: String,
        status: String,
    },
    CredentialRevoked {
        credential_id: String,
    },
    CredentialReissued {
        reissuance_id: String,
        original_credential_id: String,
        new_credential_id: String,
        offer_id: String,
    },
}

impl IntegrationEvent for IssuanceIntegrationEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::CredentialOffered { .. } => "com.impierce.unicore.issuance.credential.offered",
            Self::TxCodeGenerated { .. } => "com.impierce.unicore.issuance.offer.tx_code_generated",
            Self::CredentialOfferEmailSent { .. } => "com.impierce.unicore.issuance.offer.email_sent",
            Self::CredentialRequestVerified { .. } => "com.impierce.unicore.issuance.credential.request_verified",
            Self::CredentialIssued { .. } => "com.impierce.unicore.issuance.credential.issued",
            Self::CredentialRevoked { .. } => "com.impierce.unicore.issuance.credential.revoked",
            Self::CredentialReissued { .. } => "com.impierce.unicore.issuance.credential.reissued",
        }
    }

    fn subject(&self) -> Option<String> {
        match self {
            Self::CredentialOffered { offer_id, .. }
            | Self::TxCodeGenerated { offer_id, .. }
            | Self::CredentialOfferEmailSent { offer_id, .. }
            | Self::CredentialRequestVerified { offer_id, .. } => Some(offer_id.clone()),
            Self::CredentialIssued { credential_id, .. } | Self::CredentialRevoked { credential_id } => {
                Some(credential_id.clone())
            }
            Self::CredentialReissued { reissuance_id, .. } => Some(reissuance_id.clone()),
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
                tx_code,
                delivery_options,
                ..
            } => {
                let credential_ids = match credential_offer {
                    oid4vci::credential_offer::CredentialOffer::CredentialOffer(payload) => {
                        payload.credential_configuration_ids.iter().cloned().collect()
                    }
                    _ => Vec::new(),
                };

                Ok(Self::CredentialOffered {
                    offer_id,
                    credential_ids,
                    pre_authorized_code: Some(pre_authorized_code),
                    tx_code,
                    delivery_options,
                })
            }
            OfferEvent::TxCodeGenerated {
                offer_id,
                tx_code,
                delivery_options,
            } => Ok(Self::TxCodeGenerated {
                offer_id,
                tx_code,
                delivery_options,
            }),
            OfferEvent::CredentialOfferEmailSent {
                offer_id,
                recipient_email,
                form_url_encoded_credential_offer,
                offer_link,
                ..
            } => Ok(Self::CredentialOfferEmailSent {
                offer_id,
                recipient_email,
                form_url_encoded_credential_offer,
                offer_link,
            }),
            OfferEvent::CredentialRequestVerified { offer_id, subject_id } => {
                Ok(Self::CredentialRequestVerified { offer_id, subject_id })
            }
            _ => Err(UnmappedEvent),
        }
    }
}

impl TryFrom<CredentialEvent> for IssuanceIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: CredentialEvent) -> Result<Self, Self::Error> {
        match event {
            CredentialEvent::SignedCredentialCreated { credential_id, .. } => Ok(Self::CredentialIssued {
                credential_id,
                status: "issued".to_string(),
            }),
            CredentialEvent::CredentialSigned {
                credential_id, status, ..
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

impl TryFrom<ReissuanceEvent> for IssuanceIntegrationEvent {
    type Error = UnmappedEvent;

    fn try_from(event: ReissuanceEvent) -> Result<Self, Self::Error> {
        match event {
            ReissuanceEvent::ReissuanceCreated {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
                ..
            } => Ok(Self::CredentialReissued {
                reissuance_id,
                original_credential_id,
                new_credential_id,
                offer_id,
            }),
        }
    }
}

impl ProjectIntegrationEvent for Offer {
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        IssuanceIntegrationEvent::try_from(envelope.payload.clone())
            .ok()?
            .into_projected_cloud_event(envelope, "/services/issuance")
            .ok()
    }
}

impl ProjectIntegrationEvent for Credential {
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        IssuanceIntegrationEvent::try_from(envelope.payload.clone())
            .ok()?
            .into_projected_cloud_event(envelope, "/services/issuance")
            .ok()
    }
}

impl ProjectIntegrationEvent for Reissuance {
    fn project_integration_event(envelope: &EventEnvelope<Self>) -> Option<CloudEvent> {
        IssuanceIntegrationEvent::try_from(envelope.payload.clone())
            .ok()?
            .into_projected_cloud_event(envelope, "/services/issuance")
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::event::CredentialEvent;
    use std::collections::HashMap;

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

    #[test]
    fn project_reissuance_created_preserves_id_and_metadata() {
        let event = ReissuanceEvent::ReissuanceCreated {
            reissuance_id: "reissue-1".to_string(),
            original_credential_id: "cred-old".to_string(),
            new_credential_id: "cred-new".to_string(),
            offer_id: "offer-1".to_string(),
            credential_configuration_id: "config-1".to_string(),
            reason: Some("expired".to_string()),
            trigger_type: None,
            triggered_by: None,
            created_at: chrono::Utc::now(),
        };
        let mut metadata = HashMap::new();
        metadata.insert("callerid".to_string(), "actor-1".to_string());
        metadata.insert("callertype".to_string(), "user".to_string());

        let envelope = EventEnvelope {
            aggregate_id: "reissue-1".to_string(),
            sequence: 1,
            payload: event,
            metadata,
        };

        let projected = Reissuance::project_integration_event(&envelope).expect("should project");
        assert_eq!(projected.id, "reissuance:reissue-1:1");
        assert!(projected.time.is_some());
        assert_eq!(
            projected.event_type,
            "com.impierce.unicore.issuance.credential.reissued"
        );
        assert_eq!(projected.source, "/services/issuance");
        assert_eq!(projected.subject.as_deref(), Some("reissue-1"));
        assert_eq!(projected.extension.callerid.as_deref(), Some("actor-1"));
    }

    #[test]
    fn try_from_tx_code_generated_and_email_sent() {
        let tx_event = OfferEvent::TxCodeGenerated {
            offer_id: "offer-1".to_string(),
            tx_code: "123456".to_string(),
            delivery_options: Some(DeliveryOptions {
                recipient_email: Some("holder@example.test".to_string()),
            }),
        };
        let integration = IssuanceIntegrationEvent::try_from(tx_event).expect("should map");
        assert_eq!(
            integration.event_type(),
            "com.impierce.unicore.issuance.offer.tx_code_generated"
        );
        assert_eq!(integration.subject().as_deref(), Some("offer-1"));
        let json = serde_json::to_value(&integration).unwrap();
        assert_eq!(json["tx_code"], "123456");
        assert_eq!(json["delivery_options"]["recipient_email"], "holder@example.test");

        let email_event = OfferEvent::CredentialOfferEmailSent {
            offer_id: "offer-1".to_string(),
            recipient_email: "holder@example.test".to_string(),
            form_url_encoded_credential_offer: "openid-credential-offer://...".to_string(),
            offer_link: Url::parse("https://example.test/public/offers/offer-1").unwrap(),
            status: crate::offer::aggregate::Status::Pending,
        };
        let email_integration = IssuanceIntegrationEvent::try_from(email_event).expect("should map");
        assert_eq!(
            email_integration.event_type(),
            "com.impierce.unicore.issuance.offer.email_sent"
        );
        assert_eq!(email_integration.subject().as_deref(), Some("offer-1"));
    }
}
