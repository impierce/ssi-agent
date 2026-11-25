use crate::{credential::aggregate::Credential, reissuance::service::Replacement};
use async_trait::async_trait;

/// Decides whether an original credential may be replaced. Issuance asks it after every other check passed and
/// before anything is written. Set once where the application is composed, in
/// [`IssuancePorts`](crate::ports::IssuancePorts); the default is [`AllowEveryReissuance`].
#[async_trait]
pub trait ReissuancePolicy: Send + Sync {
    /// `Ok(())` lets the reissuance proceed; an error refuses it, and nothing is written.
    async fn check(&self, original: &Credential, replacement: &Replacement) -> Result<(), ReissuancePolicyError>;
}

/// Why a [`ReissuancePolicy`] refused a reissuance.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReissuancePolicyError {
    #[error("Reissuance is not allowed")]
    NotAllowed,
}

/// The default [`ReissuancePolicy`]: every reissuance that passes issuance's own checks is allowed.
#[derive(Debug, Default)]
pub struct AllowEveryReissuance;

#[async_trait]
impl ReissuancePolicy for AllowEveryReissuance {
    async fn check(&self, _original: &Credential, _replacement: &Replacement) -> Result<(), ReissuancePolicyError> {
        Ok(())
    }
}
