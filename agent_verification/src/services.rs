use std::sync::Arc;

use oid4vc_core::{client_metadata::ClientMetadata, Subject};
use oid4vc_manager::RelyingPartyManager;

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub client_metadata: ClientMetadata,
}

impl VerificationServices {
    pub fn new(verifier: Arc<dyn Subject>, client_metadata: ClientMetadata) -> Self {
        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new([verifier]).unwrap(),
            client_metadata,
        }
    }
}

#[cfg(feature = "test")]
pub mod test_utils {
    use std::str::FromStr;

    use agent_shared::secret_manager::secret_manager;
    use oid4vc_core::{DidMethod, SubjectSyntaxType};

    use super::*;

    pub fn test_verification_services() -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(
            Arc::new(futures::executor::block_on(async { secret_manager().await })),
            ClientMetadata::default().with_subject_syntax_types_supported(vec![SubjectSyntaxType::Did(
                DidMethod::from_str("did:key").unwrap(),
            )]),
        ))
    }
}
