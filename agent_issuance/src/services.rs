use agent_secret_manager::{service::Service, subject::SubjectExt};
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub issuer: Arc<dyn SubjectExt>,
}

impl Service for IssuanceServices {
    fn new(issuer: Arc<dyn SubjectExt>) -> Self {
        Self { issuer }
    }
}
