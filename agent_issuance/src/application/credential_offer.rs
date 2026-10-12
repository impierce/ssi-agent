use crate::{
    offer::{command::OfferCommand, error::OfferError},
    state::IssuanceState,
};
use agent_shared::{
    config::Authorization,
    handlers::{command_handler, query_handler, CommandHandlerError, QueryHandlerError},
};
use oid4vci::credential_offer::GrantType;
use shared_kernel::authorization::Caller;

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
        let tx_code_constraints = authorization
            .pre_authorized
            .then_some(authorization.tx_code_constraints)
            .flatten();
        let grant_types = vec![if authorization.pre_authorized {
            GrantType::PreAuthorizedCode
        } else {
            GrantType::AuthorizationCode
        }];
        command_handler(
            state.authorization_checker.clone(),
            caller.clone(),
            offer_id,
            &state.command.offer,
            OfferCommand::CreateCredentialOffer {
                offer_id: offer_id.to_owned(),
                template_ids: vec![configuration_id.to_owned()],
                grant_types,
                tx_code_constraints,
                delivery_options: None,
            },
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
