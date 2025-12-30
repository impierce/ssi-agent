use agent_identity::services::ThisIsTheMainService;
use agent_secret_manager::subject::SubjectExt;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    issuer: Arc<dyn SubjectExt>,
    pub this_is_the_main_service: Arc<ThisIsTheMainService>,
}

impl IssuanceServices {
    pub fn new(issuer: Arc<dyn SubjectExt>, this_is_the_main_service: Arc<ThisIsTheMainService>) -> Self {
        Self {
            issuer,
            this_is_the_main_service,
        }
    }
}
