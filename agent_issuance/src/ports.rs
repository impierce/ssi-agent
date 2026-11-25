use crate::{
    refresh::preparation::{RefreshPreparation, RefuseEveryRefresh},
    reissuance::policy::{AllowEveryReissuance, ReissuancePolicy},
};
use std::sync::Arc;

/// The decisions issuance leaves to the application. They are set once, where the application is composed, and
/// [`IssuanceState`](crate::state::IssuanceState) holds them; nothing builds them per request.
///
/// The defaults allow every reissuance that passes issuance's own checks, and refuse every credential refresh.
#[derive(Clone)]
pub struct IssuancePorts {
    /// Asked before a credential is reissued.
    pub reissuance_policy: Arc<dyn ReissuancePolicy>,
    /// Asked what replaces a credential whose holder asks for a fresh one.
    pub refresh_preparation: Arc<dyn RefreshPreparation>,
}

impl Default for IssuancePorts {
    fn default() -> Self {
        Self {
            reissuance_policy: Arc::new(AllowEveryReissuance),
            refresh_preparation: Arc::new(RefuseEveryRefresh),
        }
    }
}
