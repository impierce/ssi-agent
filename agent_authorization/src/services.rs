use agent_secret_manager::{
    service::Service,
    subject::{Subject, SubjectExt},
};
use std::sync::Arc;

pub struct AuthorizationServices {
    pub signer: Arc<Subject>,
}

impl Service for AuthorizationServices {
    fn new(signer: Arc<Subject>) -> Self {
        Self { signer }
    }
}
