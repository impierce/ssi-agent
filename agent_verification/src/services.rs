use agent_shared::metadata::Metadata;
use jsonwebtoken::Algorithm;
use oid4vc_core::{client_metadata::ClientMetadataResource, Subject};
use oid4vc_manager::RelyingPartyManager;
use oid4vp::ClaimFormatProperty;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tracing::info;

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
    pub oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
}

impl VerificationServices {
    pub fn new(verifier: Arc<dyn Subject>, metadata: &Metadata) -> Self {
        // let default_did_method = metadata
        //     .subject_syntax_types_supported
        //     .first()
        //     .expect("`subject_syntax_types_supported` must contain at least one element.")
        //     .to_string();
        let default_did_method = "did:key".to_string();

        let client_name = metadata.display.first().as_ref().map(|display| display.name.clone());

        let logo_uri = metadata
            .display
            .first()
            .and_then(|display| display.logo.as_ref().and_then(|logo| logo.url.clone()));

        let id_token_signed_response_alg = metadata.signing_algorithms_supported.first().cloned();

        let siopv2_client_metadata = ClientMetadataResource::ClientMetadata {
            client_name: client_name.clone(),
            logo_uri: logo_uri.clone(),
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: metadata.subject_syntax_types_supported.clone(),
                id_token_signed_response_alg,
            },
            other: HashMap::from_iter([(
                "id_token_signing_alg_values_supported".to_string(),
                json!(metadata.id_token_signing_alg_values_supported),
            )]),
        };

        let oid4vp_client_metadata = ClientMetadataResource::ClientMetadata {
            client_name,
            logo_uri,
            extension: oid4vp::authorization_request::ClientMetadataParameters {
                vp_formats: metadata
                    .vp_formats
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            ClaimFormatProperty::Alg(
                                v.get("alg")
                                    .map(|value| {
                                        value
                                            .as_sequence()
                                            .unwrap()
                                            .iter()
                                            .map(|value| value.as_str().unwrap().parse().unwrap())
                                            .collect::<Vec<Algorithm>>()
                                    })
                                    .unwrap(),
                            ),
                        )
                    })
                    .collect(),
            },
            other: HashMap::from_iter([(
                "subject_syntax_types_supported".to_string(),
                json!(metadata.subject_syntax_types_supported),
            )]),
        };

        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new(
                verifier,
                default_did_method,
                metadata.signing_algorithms_supported.clone(),
            )
            .unwrap(),
            siopv2_client_metadata,
            oid4vp_client_metadata,
        }
    }
}

#[cfg(feature = "test_utils")]
pub mod test_utils {
    use agent_secret_manager::secret_manager;
    use agent_secret_manager::subject::Subject;
    use agent_shared::metadata::load_metadata;

    use super::*;

    pub fn test_verification_services() -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(
            Arc::new(futures::executor::block_on(async {
                Subject {
                    secret_manager: secret_manager().await,
                }
            })),
            &load_metadata(),
        ))
    }
}
