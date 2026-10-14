use crate::{
    offer::{aggregate::DeliveryOptions, command::OfferCommand, error::OfferError},
    state::IssuanceState,
};
use agent_shared::{
    config::Authorization,
    handlers::{command_handler, query_handler, CommandHandlerError, QueryHandlerError},
};
use oid4vci::credential_offer::GrantType;
use shared_kernel::authorization::Caller;

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

pub enum OfferCreation {
    Fresh,
    ReuseOrCreate,
}

#[derive(Debug, thiserror::Error)]
pub enum PrepareOfferError {
    #[error(transparent)]
    Query(#[from] QueryHandlerError),
    #[error(transparent)]
    Command(#[from] CommandHandlerError<OfferError>),
}

/// Prepares an offer with the credential, reusing a caller-selected offer only when requested.
pub async fn ensure_offer_contains_credential(
    state: &IssuanceState,
    caller: Caller,
    offer_id: &str,
    configuration_id: &str,
    authorization: Authorization,
    credential_id: &str,
    creation: OfferCreation,
) -> Result<(), PrepareOfferError> {
    let create = match creation {
        OfferCreation::Fresh => true,
        OfferCreation::ReuseOrCreate => query_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            offer_id,
            Some(offer_id),
            &state.query.offer,
        )
        .await?
        .is_none(),
    };
    if create {
        command_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            offer_id,
            &state.command.offer,
            create_credential_offer_command(offer_id, configuration_id, &authorization, None),
        )
        .await?;
    }
    command_handler(
        state.authorization_checker.clone(),
        caller,
        offer_id,
        &state.command.offer,
        OfferCommand::AddCredentials {
            offer_id: offer_id.to_owned(),
            credential_ids: vec![credential_id.to_owned()],
            template_ids: vec![configuration_id.to_owned()],
        },
    )
    .await?;
    Ok(())
}
