use oid4vci::credential_offer::CredentialOffer;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

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

impl CommandOperation for OfferCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::ReceiveCredentialOffer { .. } => "holder.offers.receive",
            Self::AcceptCredentialOffer { .. } => "holder.offers.accept",
            Self::SendCredentialRequest { .. } => "holder.offers.credential_request.send",
            Self::RejectCredentialOffer { .. } => "holder.offers.reject",
        }
    }
}
