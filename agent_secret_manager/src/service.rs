use std::sync::Arc;

/// Convenience trait for Services like `IssuanceServices`, `HolderServices`, and `VerifierServices`.
pub trait Service {
    fn new(subject: Arc<dyn oid4vc_core::Subject>) -> Self;

    #[cfg(feature = "test_utils")]
    fn default() -> Arc<Self>
    where
        Self: Sized,
    {
        Arc::new(Self::new(Arc::new(crate::subject::Subject::default())))
    }
}
