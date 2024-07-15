use agent_shared::config::{config, did_methods_enabled};
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
        let default_did_method = config()
            .did_methods
            .iter()
            .filter(|(_, v)| v.preferred.unwrap_or(false))
            .map(|(k, _)| k.clone())
            .collect::<Vec<String>>()
            // TODO: throw error if more than one preferred DID method is found
            .first()
            .unwrap()
            .to_owned()
            .replace("_", ":");

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

        // let id_token_signed_response_alg = signing_algorithms_supported.first().cloned();

        let siopv2_client_metadata = ClientMetadataResource::ClientMetadata {
            client_name: client_name.clone(),
            logo_uri: logo_uri.clone(),
            extension: siopv2::authorization_request::ClientMetadataParameters {
                subject_syntax_types_supported: did_methods_enabled()
                    .iter()
                    .map(|method| oid4vc_core::SubjectSyntaxType::from_str(method).unwrap())
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
                // vp_formats: metadata
                //     .vp_formats
                //     .iter()
                //     .map(|(k, v)| {
                //         (
                //             k.clone(),
                //             ClaimFormatProperty::Alg(
                //                 v.get("alg")
                //                     .map(|value| {
                //                         value
                //                             .as_sequence()
                //                             .unwrap()
                //                             .iter()
                //                             .map(|value| value.as_str().unwrap().parse().unwrap())
                //                             .collect::<Vec<Algorithm>>()
                //                     })
                //                     .unwrap(),
                //             ),
                //         )
                //     })
                //     .collect(),
            },
            other: HashMap::from_iter([(
                "subject_syntax_types_supported".to_string(),
                json!(did_methods_enabled()),
            )]),
        };

        Self {
            verifier: verifier.clone(),
            relying_party: RelyingPartyManager::new(verifier, default_did_method, signing_algorithms_supported)
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
