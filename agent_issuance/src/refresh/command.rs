use crate::refresh::aggregate::Continuation;
use serde::Deserialize;
use shared_kernel::authorization::CommandOperation;

/// Commands on a [`RefreshCapability`](crate::refresh::aggregate::RefreshCapability). The refresh token is the only
/// authorization of a refresh, so these run without an authorization check, and the token never appears in them:
/// a capability is addressed by the token's hash.
#[derive(Debug, Deserialize)]
pub enum RefreshCapabilityCommand {
    /// Creates the capability of a credential that embeds a refresh token. Refused when it already exists.
    CreateRefreshCapability {
        credential_id: String,
        credential_configuration_id: String,
    },
    /// Records a replacement prepared from the capability as its newest continuation, the one whose offer a repeat
    /// refresh answers with until it is delivered.
    ///
    /// `after` is the reissuance of the newest continuation the caller saw (`None`: none). When another refresh
    /// recorded a continuation since, this one is refused, so concurrent refreshes leave a single pending offer.
    RecordContinuation {
        after: Option<String>,
        continuation: Continuation,
    },
    /// Retires the capability because the holder accepted `accepted_credential_id`, one of its continuations.
    /// Retiring a retired capability changes nothing.
    RetireRefreshCapability { accepted_credential_id: String },
}

impl CommandOperation for RefreshCapabilityCommand {
    fn operation_name(&self) -> &'static str {
        match self {
            Self::CreateRefreshCapability { .. } => "issuance.refresh_capabilities.create",
            Self::RecordContinuation { .. } => "issuance.refresh_capabilities.continuations.record",
            Self::RetireRefreshCapability { .. } => "issuance.refresh_capabilities.retire",
        }
    }
}
