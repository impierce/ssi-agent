use agent_shared::config::{config, get_all_enabled_did_methods, get_preferred_did_method};
use jsonwebtoken::Algorithm;
use oid4vc_core::{client_metadata::ClientMetadataResource, Subject};
use oid4vc_manager::RelyingPartyManager;
use oid4vp::ClaimFormatProperty;
use serde_json::json;
use std::{collections::HashMap, str::FromStr, sync::Arc};

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<dyn Subject>,
    pub relying_party: RelyingPartyManager,
    pub siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
    pub oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
}

impl VerificationServices {
    pub fn new(verifier: Arc<dyn Subject>) -> Self {
        let client_name = config().display.first().as_ref().map(|display| display.name.clone());

        let logo_uri = config()
            .display
            .first()
            .and_then(|display| display.logo.as_ref().and_then(|logo| logo.url.clone()));

        let signing_algorithms_supported: Vec<Algorithm> = config()
            .signing_algorithms_supported
            .iter()
            .filter(|(_, opts)| opts.enabled)
            .map(|(alg, _)| *alg)
            .collect();

        let siopv2_client_metadata = ClientMetadataResource::ClientMetadata {
            client_name: client_name.clone(),
            logo_uri: logo_uri.clone(),
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: get_all_enabled_did_methods()
                    .iter()
                    .map(|method| oid4vc_core::SubjectSyntaxType::from_str(&method.to_string()).unwrap())
                    .collect(),
                id_token_signed_response_alg: signing_algorithms_supported.first().cloned(),
            },
            other: HashMap::from_iter([(
                "id_token_signing_alg_values_supported".to_string(),
                json!(signing_algorithms_supported),
            )]),
        };

        let oid4vp_client_metadata = ClientMetadataResource::ClientMetadata {
            client_name,
            logo_uri,
            extension: oid4vp::authorization_request::ClientMetadataParameters {
                vp_formats: config()
                    .vp_formats
                    .iter()
                    .filter(|(_, opts)| opts.enabled)
                    .map(|(c, _)| {
                        (
                            c.clone(),
                            ClaimFormatProperty::Alg(signing_algorithms_supported.clone()),
                        )
                    })
                    .collect(),
            },
            other: HashMap::from_iter([(
                "subject_syntax_types_supported".to_string(),
                json!(get_all_enabled_did_methods()),
            )]),
        };

        let default_subject_syntax_type = get_preferred_did_method();

        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new(
                verifier,
                default_subject_syntax_type.to_string(),
                signing_algorithms_supported,
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

    use super::*;

    pub fn test_verification_services() -> Arc<VerificationServices> {
        Arc::new(VerificationServices::new(Arc::new(futures::executor::block_on(
            async {
                Subject {
                    secret_manager: secret_manager().await,
                }
            },
        ))))
    }
}
