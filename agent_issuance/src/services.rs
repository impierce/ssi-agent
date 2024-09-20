use agent_secret_manager::service::Service;
use oid4vc_core::Subject;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub issuer: Arc<dyn Subject>,
}

impl Service for IssuanceServices {
    fn new(issuer: Arc<dyn Subject>) -> Self {
        Self { issuer }
    }
}
