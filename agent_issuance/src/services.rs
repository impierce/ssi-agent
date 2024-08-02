use oid4vc_core::Subject;
use std::sync::Arc;

/// Issuance services. This struct is used to sign credentials and validate credential requests.
pub struct IssuanceServices {
    pub issuer: Arc<dyn Subject>,
}

impl IssuanceServices {
    pub fn new(issuer: Arc<dyn Subject>) -> Self {
        Self { issuer }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;

    use super::*;

    pub fn test_issuance_services() -> Arc<IssuanceServices> {
        Arc::new(IssuanceServices::new(Arc::new(futures::executor::block_on(async {
            Subject {
                secret_manager: secret_manager().await,
            }
        }))))
    }
}
