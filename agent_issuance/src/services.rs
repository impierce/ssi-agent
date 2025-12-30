use agent_identity::services::ThisIsTheMainService;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub this_is_the_main_service: Arc<ThisIsTheMainService>,
}

impl IssuanceServices {
    pub fn new(this_is_the_main_service: Arc<ThisIsTheMainService>) -> Self {
        Self {
            this_is_the_main_service,
        }
    }
}
