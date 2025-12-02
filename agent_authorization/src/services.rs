use agent_secret_manager::{service::Service, subject::SubjectExt};
use std::sync::Arc;

pub struct AuthorizationServices {
    pub signer: Arc<dyn SubjectExt>,
}

impl Service for AuthorizationServices {
    fn new(signer: Arc<dyn SubjectExt>) -> Self {
        Self { signer }
    }
}
