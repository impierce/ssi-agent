use agent_identity::services::ThisIsTheMainService;
use std::sync::Arc;

pub struct AuthorizationServices {
    pub this_is_the_main_service: Arc<ThisIsTheMainService>,
}

impl AuthorizationServices {
    pub fn new(this_is_the_main_service: Arc<ThisIsTheMainService>) -> Self {
        Self {
            this_is_the_main_service,
        }
    }
}
