use agent_identity::services::ThisIsTheMainService;
use agent_secret_manager::{service::Service, subject::SubjectExt};
use std::sync::Arc;

pub struct AuthorizationServices {
    signer: Arc<dyn SubjectExt>,
    pub this_is_the_main_service: Arc<ThisIsTheMainService>,
}

impl AuthorizationServices {
    pub fn new(signer: Arc<dyn SubjectExt>, this_is_the_main_service: Arc<ThisIsTheMainService>) -> Self {
        Self {
            signer,
            this_is_the_main_service,
        }
    }
}
