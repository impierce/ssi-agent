use crate::offer::{aggregate::DeliveryOptions, command::OfferCommand};
use agent_shared::config::Authorization;
use oid4vci::credential_offer::GrantType;

/// Derives an offer's grants and transaction-code constraints from the template authorization.
pub fn create_credential_offer_command(
    offer_id: &str,
    configuration_id: &str,
    authorization: &Authorization,
    delivery_options: Option<DeliveryOptions>,
) -> OfferCommand {
    let tx_code_constraints = authorization
        .pre_authorized
        .then(|| authorization.tx_code_constraints.clone())
        .flatten();
    let grant_types = vec![if authorization.pre_authorized {
        GrantType::PreAuthorizedCode
    } else {
        GrantType::AuthorizationCode
    }];
    OfferCommand::CreateCredentialOffer {
        offer_id: offer_id.to_owned(),
        template_ids: vec![configuration_id.to_owned()],
        grant_types,
        tx_code_constraints,
        delivery_options,
    }
}
