use agent_shared::handlers::{command_handler, CommandHandlerError};
use shared_kernel::authorization::Caller;

use crate::{
    credential::{command::CredentialCommand, error::CredentialError},
    state::IssuanceState,
    status_list::application::index_allocation::{add_index, status_list_with_room, IndexAllocationError},
};

/// Decides whether the Credentials created from a template are signed when they are created.
///
/// Otherwise a Credential is signed, and given its status-list index, only when a wallet redeems its
/// Offer.
#[async_trait::async_trait]
pub trait SigningAtCreationPolicy: Send + Sync {
    async fn signs_at_creation(&self, template_id: &str) -> bool;
}

/// Signs no Credential at creation.
pub struct SignOnRedemption;

#[async_trait::async_trait]
impl SigningAtCreationPolicy for SignOnRedemption {
    async fn signs_at_creation(&self, _template_id: &str) -> bool {
        false
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SigningAtCreationError {
    #[error(transparent)]
    IndexAllocation(#[from] IndexAllocationError),
    #[error(transparent)]
    Credential(#[from] CommandHandlerError<CredentialError>),
}

/// Signs an unsigned Credential without holder binding and gives it a status-list index, so it is
/// Issued without an Offer being redeemed.
pub async fn sign_at_creation(state: &IssuanceState, credential_id: &str) -> Result<(), SigningAtCreationError> {
    let status_list_id = status_list_with_room(state, 1).await?;
    let index = add_index(state, &status_list_id).await?;

    let command = CredentialCommand::SignCredential {
        credential_id: credential_id.to_owned(),
        subject_id: None,
        overwrite: false,
        proofs: None,
        status_list_id,
        index,
    };
    command_handler(
        state.authorization_checker.clone(),
        Caller::Internal,
        credential_id,
        &state.command.credential,
        command,
    )
    .await?;

    Ok(())
}
