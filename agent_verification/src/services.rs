use std::sync::Arc;

use oid4vc_core::{client_metadata::ClientMetadataResource, Subject};
use oid4vc_manager::RelyingPartyManager;

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
    pub oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
}

impl VerificationServices {
    pub fn new(
        verifier: Arc<dyn Subject>,
        siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
        oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
        default_did_method: &str,
    ) -> Self {
        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new(verifier, default_did_method).unwrap(),
            siopv2_client_metadata,
            oid4vp_client_metadata,
        }
    }
}

#[cfg(feature = "test")]
pub mod test_utils {
    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;
    use oid4vc_core::SubjectSyntaxType;
    use serde_json::json;
    use std::str::FromStr;

    use super::*;

    pub fn test_verification_services(default_did_method: &str) -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(
            Arc::new(futures::executor::block_on(async {
                Subject {
                    secret_manager: secret_manager().await,
                }
            })),
            ClientMetadataResource::ClientMetadata {
                client_name: None,
                logo_uri: None,
                extension: siopv2::authorization_request::ClientMetadataParameters {
                    subject_syntax_types_supported: vec![SubjectSyntaxType::from_str(default_did_method).unwrap()],
                },
            },
            ClientMetadataResource::ClientMetadata {
                client_name: None,
                logo_uri: None,
                // TODO: fix this once `vp_formats` is public.
                extension: serde_json::from_value(json!({
                    "vp_formats": {}
                }))
                .unwrap(),
            },
            default_did_method,
        ))
    }
}
