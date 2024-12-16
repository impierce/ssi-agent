use agent_secret_manager::subject::Subject;
use std::sync::Arc;

/// Identity services.
pub struct IdentityServices {
    pub subject: Arc<Subject>,
}

impl IdentityServices {
    pub fn new(subject: Arc<Subject>) -> Self {
        Self { subject }
    }

    #[cfg(feature = "test_utils")]
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        use agent_secret_manager::secret_manager;

        Arc::new(Self::new(Arc::new(futures::executor::block_on(async {
            Subject {
                secret_manager: Arc::new(tokio::sync::Mutex::new(secret_manager().await)),
            }
        }))))
    }
}
