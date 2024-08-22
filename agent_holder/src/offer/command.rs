use oid4vci::credential_offer::CredentialOffer;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OfferCommand {
    ReceiveCredentialOffer {
        offer_id: String,
        credential_offer: CredentialOffer,
    },
    AcceptCredentialOffer {
        offer_id: String,
    },
    SendTokenRequest {
        offer_id: String,
    },
    SendCredentialRequest {
        offer_id: String,
    },
    RejectCredentialOffer {
        offer_id: String,
    },
}
