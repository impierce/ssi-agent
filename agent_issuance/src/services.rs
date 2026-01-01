use agent_identity::services::IdentityApplicationService;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub identity_application_service: Arc<IdentityApplicationService>,
}

impl IssuanceServices {
    pub fn new(identity_application_service: Arc<IdentityApplicationService>) -> Self {
        Self {
            identity_application_service,
        }
    }
}
