use super::aggregate::Status;
use crate::offer::aggregate::DeliveryOptions;
use cqrs_es::DomainEvent;
use oid4vci::{
    credential_offer::{CredentialOffer, GrantType},
    credential_response::CredentialResponse,
};
use serde::{Deserialize, Serialize};
use strum::Display;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Display)]
pub enum OfferEvent {
    CredentialOfferCreated {
        offer_id: String,
        grant_types: Vec<GrantType>,
        credential_offer: CredentialOffer,
        credential_offer_uri: CredentialOffer,
        pre_authorized_code: String,
        status: Status,
        tx_code: Option<String>,
        delivery_options: Option<DeliveryOptions>,
    },
    CredentialsAdded {
        offer_id: String,
        credential_ids: Vec<String>,
        credential_offer: CredentialOffer,
    },
    FormUrlEncodedCredentialOfferCreated {
        offer_id: String,
        form_url_encoded_credential_offer: String,
        status: Status,
    },
    CredentialOfferSent {
        offer_id: String,
        target_url: Url,
        status: Status,
    },
    CredentialOfferEmailSent {
        offer_id: String,
        recipient_email: String,
        form_url_encoded_credential_offer: String,
        offer_link: Url,
        status: Status,
    },
    CredentialRequestVerified {
        offer_id: String,
        subject_id: Option<String>,
    },
    CredentialResponseCreated {
        offer_id: String,
        credential_response: CredentialResponse,
        status: Status,
    },
    TxCodeGenerated {
        offer_id: String,
        tx_code: String,
        delivery_options: Option<DeliveryOptions>,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    // Integer schema version of this event payload. Bump on breaking change and add an upcaster (see docs/event-versioning.md).
    fn event_version(&self) -> String {
        "1".to_string()
    }
}

/// Upcasters migrating old persisted versions of these events to the current
/// schema version. See `docs/event-versioning.md`.
pub fn upcasters() -> Vec<Box<dyn cqrs_es::persist::EventUpcaster>> {
    vec![]
}
