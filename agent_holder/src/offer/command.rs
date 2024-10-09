use oid4vci::credential_offer::CredentialOffer;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    ReceiveCredentialOffer {
        received_offer_id: String,
        credential_offer: CredentialOffer,
    },
    AcceptCredentialOffer {
        received_offer_id: String,
    },
    SendCredentialRequest {
        received_offer_id: String,
    },
    RejectCredentialOffer {
        received_offer_id: String,
    },
}
