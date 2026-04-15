use crate::subject::Subject;
use async_trait::async_trait;
use std::sync::Arc;

/// Convenience trait for Services like `IssuanceServices`, `HolderServices`, and `VerifierServices`.
#[async_trait]
pub trait Service {
    fn new(subject: Arc<Subject>) -> Self;

    #[cfg(feature = "test_utils")]
    async fn default() -> Self
    where
        Self: Sized,
    {
        Self::new(Arc::new(crate::subject::Subject::test_subject().await))
    }
}
