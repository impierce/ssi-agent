use agent_secret_manager::{service::Service, subject::Subject};
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub issuer: Arc<Subject>,
}

impl Service for IssuanceServices {
    fn new(issuer: Arc<Subject>) -> Self {
        Self { issuer }
    }
}
