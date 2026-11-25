use crate::{credential::aggregate::Credential, reissuance::service::Replacement};
use async_trait::async_trait;

/// Decides what replaces a credential whose holder asks for a fresh one: its configuration, content, expiry and
/// reissuance trigger. Set once where the application is composed, in
/// [`IssuancePorts`](crate::ports::IssuancePorts); the default, [`RefuseEveryRefresh`], refuses every refresh.
///
/// While the replacement it prepared is not delivered, a repeat refresh answers with the same offer without asking
/// again. Concurrent refreshes with one token can each ask, and only one answer becomes the continuation, so
/// `prepare` must be free of side effects the consumer relies on.
#[async_trait]
pub trait RefreshPreparation: Send + Sync {
    /// Prepares the replacement of `original`, which was issued with `credential_configuration_id`.
    async fn prepare(
        &self,
        original: &Credential,
        credential_configuration_id: &str,
    ) -> Result<Replacement, RefreshPreparationError>;
}

/// Why a [`RefreshPreparation`] prepared no replacement.
#[derive(Debug, thiserror::Error)]
pub enum RefreshPreparationError {
    /// The credential can't be refreshed now. The holder is told the refresh is unavailable (403).
    #[error("The credential cannot be refreshed")]
    Refused,
    /// Preparing the replacement failed. The holder is told it failed (500), and the cause is logged.
    #[error("Preparing the replacement failed: {0}")]
    Failed(anyhow::Error),
}

/// The default [`RefreshPreparation`]: refuses every refresh.
#[derive(Debug, Default)]
pub struct RefuseEveryRefresh;

#[async_trait]
impl RefreshPreparation for RefuseEveryRefresh {
    async fn prepare(&self, _original: &Credential, _: &str) -> Result<Replacement, RefreshPreparationError> {
        Err(RefreshPreparationError::Refused)
    }
}
