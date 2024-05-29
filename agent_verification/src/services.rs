use std::sync::Arc;

use jsonwebtoken::Algorithm;
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
            relying_party: RelyingPartyManager::new(verifier, default_did_method, vec![Algorithm::EdDSA]).unwrap(),
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
    use oid4vp::{ClaimFormatDesignation, ClaimFormatProperty};
    use serde_json::json;
    use std::{collections::HashMap, str::FromStr};

    use super::*;

    pub fn test_verification_services(default_did_method: &str) -> Arc<VerificationServices> {
        let default_did_methods = vec![
            SubjectSyntaxType::from_str("did:key").unwrap(),
            SubjectSyntaxType::from_str("did:jwk").unwrap(),
            SubjectSyntaxType::from_str("did:iota:rms").unwrap(),
        ];
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
                    subject_syntax_types_supported: default_did_methods.clone(),
                    id_token_signed_response_alg: Some(Algorithm::EdDSA),
                },
                other: HashMap::default(),
            },
            ClientMetadataResource::ClientMetadata {
                client_name: None,
                logo_uri: None,
                extension: oid4vp::authorization_request::ClientMetadataParameters {
                    vp_formats: vec![(
                        ClaimFormatDesignation::JwtVcJson,
                        ClaimFormatProperty::Alg(vec![Algorithm::EdDSA]),
                    )]
                    .into_iter()
                    .collect(),
                },
                other: HashMap::from_iter(vec![(
                    "subject_syntax_types_supported".to_string(),
                    json!(&default_did_methods),
                )]),
            },
            default_did_method,
        ))
    }
}
