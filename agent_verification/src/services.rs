use std::sync::Arc;

use oid4vc_core::{client_metadata::ClientMetadataResource, Subject};
use oid4vc_manager::RelyingPartyManager;
use siopv2::authorization_request::ClientMetadataParameters;

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub client_metadata: ClientMetadataResource<ClientMetadataParameters>,
}

impl VerificationServices {
    pub fn new(verifier: Arc<dyn Subject>, client_metadata: ClientMetadataResource<ClientMetadataParameters>) -> Self {
        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new(verifier, "did:key").unwrap(),
            client_metadata,
        }
    }
}

#[cfg(feature = "test")]
pub mod test_utils {
    use std::str::FromStr;

    use agent_secret_manager::secret_manager;
    use oid4vc_core::{DidMethod, SubjectSyntaxType};
    use siopv2::authorization_request::ClientMetadataParameters;

    use super::*;

    pub fn test_verification_services() -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(
            Arc::new(futures::executor::block_on(async { secret_manager().await })),
            ClientMetadataResource::ClientMetadata {
                client_name: None,
                logo_uri: None,
                extension: ClientMetadataParameters {
                    subject_syntax_types_supported: vec![SubjectSyntaxType::Did(
                        DidMethod::from_str("did:key").unwrap(),
                    )],
                },
            },
        ))
    }
}
