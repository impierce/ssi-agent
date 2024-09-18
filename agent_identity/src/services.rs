use agent_secret_manager::subject::Subject;
use std::sync::Arc;

/// Identity services. This struct is used to sign credentials and validate credential requests.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>) -> Self {
        Self { subject }
    }
}
