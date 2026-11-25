use crate::reissuance::policy::{AllowEveryReissuance, ReissuancePolicy};
use std::sync::Arc;

/// The decisions issuance leaves to the application. They are set once, where the application is composed, and
/// [`IssuanceState`](crate::state::IssuanceState) holds them; nothing builds them per request.
///
/// The default allows every reissuance that passes issuance's own checks.
#[derive(Clone)]
pub struct IssuancePorts {
    /// Asked before a credential is reissued.
    pub reissuance_policy: Arc<dyn ReissuancePolicy>,
}

impl Default for IssuancePorts {
    fn default() -> Self {
        Self {
            reissuance_policy: Arc::new(AllowEveryReissuance),
        }
    }
}
