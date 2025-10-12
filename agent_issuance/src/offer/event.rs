use super::aggregate::Status;
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
    CredentialRequestVerified {
        offer_id: String,
        subject_id: String,
    },
    CredentialResponseCreated {
        offer_id: String,
        credential_response: CredentialResponse,
        status: Status,
    },
}

impl DomainEvent for OfferEvent {
    fn event_type(&self) -> String {
        self.to_string()
    }

    fn event_version(&self) -> String {
        "1".to_string()
    }
}
