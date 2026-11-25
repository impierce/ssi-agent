use crate::{
    application::template_issuance::TemplateIssuanceError, credential::error::CredentialError,
    reissuance::policy::ReissuancePolicyError,
};
use cqrs_es::{persist::PersistenceError, AggregateError};
use shared_kernel::authorization::AuthorizationError;
use thiserror::Error;

/// Why a reissuance was not prepared. Shared by the reissuance relation aggregate, which only ever refuses with
/// [`ReissuanceError::AlreadyExists`], and by [`reissue_credential`](crate::reissuance::service::reissue_credential).
#[derive(Error, Debug, strum::IntoStaticStr)]
pub enum ReissuanceError {
    /// The original credential does not exist.
    #[error("Original credential `{0}` was not found")]
    OriginalCredentialNotFound(String),
    /// Only an issued credential can be replaced: a pending one has not reached its holder yet.
    #[error("Only issued credentials can be reissued")]
    OriginalCredentialNotIssued,
    /// The replacement names a credential configuration that does not exist.
    #[error("Credential configuration `{0}` was not found")]
    CredentialConfigurationNotFound(String),
    /// The replacement's configuration issues another format than the original.
    #[error("The requested configuration must match the original credential configuration")]
    CredentialConfigurationMismatch,
    /// Only `dc+sd-jwt` and `vc+sd-jwt` credentials can be reissued.
    #[error("Credential format is not supported for reissuance: {0}")]
    UnsupportedCredentialFormat(serde_json::Value),
    /// The replacement's content is not a JSON object.
    #[error("Credential payload must be a JSON object")]
    InvalidCredentialPayload,
    /// The replacement's template is not published, or the content or expiry does not fit it.
    #[error(transparent)]
    Template(TemplateIssuanceError),
    /// The configured [`ReissuancePolicy`](crate::reissuance::policy::ReissuancePolicy) refused the reissuance.
    #[error(transparent)]
    NotAllowed(ReissuancePolicyError),
    /// The credential aggregate refused the replacement's content.
    #[error(transparent)]
    ReplacementRejected(CredentialError),
    /// The replacement's configuration opts into refresh and its refresh capability could not be created, so the
    /// replacement was not created.
    #[error("Failed to create the refresh capability of the replacement")]
    RefreshCapabilityFailed,
    /// A relation with this identifier was already recorded.
    #[error("Reissuance relation already exists")]
    AlreadyExists,
    /// The caller may not take one of the steps of a reissuance.
    #[error(transparent)]
    Authorization(AuthorizationError),
    /// The event store or a view failed, or another write to the same aggregate came first.
    #[error(transparent)]
    Persistence(PersistenceError),
    /// A step failed in a way the steps before it rule out, for example the new offer refusing its credential.
    #[error("Failed to prepare the reissuance: {0}")]
    Failed(String),
}

impl ReissuanceError {
    /// Folds a failed aggregate command into this vocabulary. A refusal by the aggregate goes through `rejected`;
    /// every other failure is a persistence failure.
    pub(crate) fn from_aggregate<E: std::error::Error>(
        error: AggregateError<E>,
        rejected: impl FnOnce(E) -> Self,
    ) -> Self {
        let persistence = match error {
            AggregateError::UserError(error) => return rejected(error),
            AggregateError::AggregateConflict => PersistenceError::OptimisticLockError,
            AggregateError::DatabaseConnectionError(error) => PersistenceError::ConnectionError(error),
            AggregateError::DeserializationError(error) => PersistenceError::DeserializationError(error),
            AggregateError::UnexpectedError(error) => PersistenceError::UnknownError(error),
        };
        Self::Persistence(persistence)
    }
}
