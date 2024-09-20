use std::sync::Arc;

/// Convenience trait for Services like `IssuanceServices`, `HolderServices`, and `VerifierServices`.
pub trait Service {
    fn new(subject: Arc<dyn oid4vc_core::Subject>) -> Self;

    #[cfg(feature = "test_utils")]
    fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        use crate::{secret_manager, subject::Subject};

        Arc::new(Self::new(Arc::new(futures::executor::block_on(async {
            Subject {
                secret_manager: Arc::new(tokio::sync::Mutex::new(secret_manager().await)),
            }
        }))))
    }
}
