use agent_secret_manager::{service::Service, subject::Subject};
use agent_shared::{
    config::{
        config, get_all_enabled_did_methods, get_all_enabled_signing_algorithms_supported, get_preferred_did_method,
    },
    credential_status_checker::CredentialStatusChecker,
};
use oid4vc_core::client_metadata::ClientMetadataResource;
use oid4vc_manager::RelyingPartyManager;
use serde_json::json;
use std::{collections::HashMap, str::FromStr, sync::Arc};

/// Verification services. This struct is used to generate authorization requests and validate authorization responses.
pub struct VerificationServices {
    pub verifier: Arc<Subject>,
    pub relying_party: RelyingPartyManager,
    pub credential_status_checker: CredentialStatusChecker,
    pub siopv2_client_metadata: ClientMetadataResource<siopv2::authorization_request::ClientMetadataParameters>,
    pub oid4vp_client_metadata: ClientMetadataResource<oid4vp::authorization_request::ClientMetadataParameters>,
}

impl Service for VerificationServices {
    fn new(verifier: Arc<Subject>) -> Self {
        let client_name = config().display.first().as_ref().map(|display| display.name.clone());

        let logo_uri = config()
            .display
            .first()
            .and_then(|display| display.logo.as_ref().and_then(|logo| logo.uri.clone()));

        let signing_algorithms_supported = get_all_enabled_signing_algorithms_supported();

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
                vp_formats_supported: config().vp_formats_supported.clone(),
                encrypted_response_enc_values_supported: None,
                jwks: None,
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
                verifier.clone(),
                default_subject_syntax_type.to_string(),
                signing_algorithms_supported,
            )
            .unwrap(),
            siopv2_client_metadata,
            oid4vp_client_metadata,
            credential_status_checker: CredentialStatusChecker {
                verification_material_resolver: verifier.clone(),
            },
        }
    }
}
