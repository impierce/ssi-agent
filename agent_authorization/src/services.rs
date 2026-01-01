use agent_identity::services::IdentityApplicationService;
use std::sync::Arc;

pub struct AuthorizationServices {
    pub identity_application_service: Arc<IdentityApplicationService>,
}

impl AuthorizationServices {
    pub fn new(identity_application_service: Arc<IdentityApplicationService>) -> Self {
        Self {
            identity_application_service,
        }
    }
}
