use crate::{stronghold_storage, subject::SubjectExt};
use did_manager_identity_stronghold_ext::StrongholdExtStorage;
use std::sync::Arc;

/// Convenience trait for Services like `IssuanceServices`, `HolderServices`, and `VerifierServices`.
pub trait Service {
    fn new(subject: Arc<dyn SubjectExt>) -> Self;

    // #[cfg(feature = "test_utils")]
    // fn default() -> Arc<Self>
    // where
    //     Self: Sized,
    // {
    //     Arc::new(Self::new(Arc::new(crate::subject::Subject::default())))
    // }
}

pub struct SecretManagerServices {
    pub stronghold_storage: StrongholdExtStorage,
}

impl SecretManagerServices {
    pub fn new(stronghold_storage: StrongholdExtStorage) -> Self {
        Self { stronghold_storage }
    }
}
